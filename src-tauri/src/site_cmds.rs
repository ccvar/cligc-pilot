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

/// 在 Cloudflare 里找到的部署：哪个连接下的哪个 Pages 项目、怎么部署。
#[derive(serde::Serialize, Clone, Debug)]
pub struct DetectedDeploy {
    pub conn_id: String,
    pub conn_name: String,
    /// pages_git（Pages 绑定了 GitHub：推送到生产分支即上线）| direct（直接上传型 Pages 项目）
    pub mode: String,
    pub project: String,
    pub branch: String,
    /// 项目绑定的 GitHub 仓库（绑定 GitHub 时）
    pub source_repo: String,
    /// 怎么认出来的：domain（自定义域名）| repo（绑定的仓库就是这个站点的仓库）
    pub matched_by: String,
}

/// 这个站点的 GitHub 仓库：GitHub 来源用 repo，本地文件夹看 origin。
fn site_github_repo(site: &crate::sites::Site) -> String {
    if !site.repo.is_empty() {
        return site.repo.clone();
    }
    crate::sites::git_out(std::path::Path::new(&site.local_dir), &["remote", "get-url", "origin"])
        .and_then(|u| crate::sites::github_repo_from_url(&u))
        .unwrap_or_default()
}

/// 每个 Cloudflare 连接下的 Pages 项目（读不到的连接跳过）。
async fn all_pages_projects(conns: &crate::conns::ConnStore) -> Vec<(crate::conns::Connection, Vec<crate::cf::PagesProject>)> {
    let mut out = Vec::new();
    for c in conns.list().into_iter().filter(|c| c.kind == "cloudflare" && !c.account_id.is_empty()) {
        let Ok(token) = crate::keychain::get_key(&c.id) else { continue };
        if let Ok(p) = crate::cf::list_pages_projects(&token, &c.account_id).await {
            out.push((c, p));
        }
    }
    out
}

/// 在这些项目里找这个站点：自定义域名对得上，或者项目绑定的仓库就是这个站点的仓库。
fn match_cf_deploy(all: &[(crate::conns::Connection, Vec<crate::cf::PagesProject>)], site: &crate::sites::Site) -> Option<DetectedDeploy> {
    let bare = site.domain.trim().trim_start_matches("www.").to_ascii_lowercase();
    let repo = site_github_repo(site).to_ascii_lowercase();
    if bare.is_empty() && repo.is_empty() {
        return None;
    }
    for (c, projects) in all {
        let by_domain = projects.iter().find(|p| !bare.is_empty() && p.domains.iter().any(|d| d.trim_start_matches("www.").eq_ignore_ascii_case(&bare)));
        let (p, how) = match by_domain.map(|p| (p, "domain")).or_else(|| projects.iter().find(|p| !repo.is_empty() && p.source_repo.eq_ignore_ascii_case(&repo)).map(|p| (p, "repo"))) {
            Some(x) => x,
            None => continue,
        };
        // 绑定了 GitHub、而且绑的就是这个站点的仓库：推送即上线；否则当直接上传型项目
        let git = p.source_type == "github" && !p.source_repo.is_empty() && p.source_repo.eq_ignore_ascii_case(&repo);
        return Some(DetectedDeploy {
            conn_id: c.id.clone(),
            conn_name: if c.remark.trim().is_empty() { c.name.clone() } else { c.remark.clone() },
            mode: if git { "pages_git".into() } else { "direct".into() },
            project: p.name.clone(),
            branch: if p.production_branch.is_empty() { site.default_branch.clone() } else { p.production_branch.clone() },
            source_repo: p.source_repo.clone(),
            matched_by: how.into(),
        });
    }
    None
}

/// 在 Cloudflare 里识别这个站点的部署（站点设置里「从 Cloudflare 识别」）。
#[tauri::command]
pub async fn detect_cf_deploy(state: tauri::State<'_, AppState>, id: String) -> Result<Option<DetectedDeploy>, String> {
    let site = state.sites.get(&id)?;
    Ok(match_cf_deploy(&all_pages_projects(&state.conns).await, &site))
}

/// 没配置部署的站点，在 Cloudflare 里按域名 / 仓库识别出来的直接填上（只填没配置的，不改你设过的）。返回填了几个。
#[tauri::command]
pub async fn autodetect_cf_deploys(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    let mut filled = Vec::new();
    let todo: Vec<crate::sites::Site> = state.sites.list().into_iter().filter(|s| s.deploy.mode == "none" && !s.domain.is_empty()).collect();
    if todo.is_empty() {
        return Ok(filled);
    }
    let all = all_pages_projects(&state.conns).await;
    for site in todo {
        let Some(d) = match_cf_deploy(&all, &site) else { continue };
        let ok = state.sites.update(&site.id, crate::now_secs(), |s| {
            if s.deploy.mode == "none" {
                s.deploy.mode = d.mode.clone();
                s.deploy.cf_conn_id = d.conn_id.clone();
                s.deploy.project = d.project.clone();
                s.deploy.branch = d.branch.clone();
            }
            Ok(())
        });
        if ok.is_ok() {
            filled.push(format!("{}：Pages 项目 {}（{}）", site.name, d.project, if d.mode == "pages_git" { "绑定 GitHub" } else { "直接上传" }));
        }
    }
    Ok(filled)
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
    site.build.root = site.build.root.trim().trim_matches('/').to_string();
    sites::check_root(&site.build.root)?;
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
    site.deploy.env_file = site.deploy.env_file.trim().to_string();
    let f = &site.deploy.env_file;
    if !f.is_empty() && (!f.starts_with('/') || f.contains("..") || f.ends_with('/') || f.chars().any(|c| c.is_control() || c == '\'')) {
        return Err("服务器上的配置文件要写绝对路径，如 /opt/mysite/config/mysite.env".into());
    }
    match site.deploy.mode.as_str() {
        "none" => {}
        "vps" => {
            let c = state.conns.get(&site.deploy.ssh_conn_id).map_err(|_| "请选择部署用的 SSH 连接".to_string())?;
            if c.kind != "ssh" {
                return Err("VPS 部署要选一条远程（SSH）连接".into());
            }
            sites::check_command(&site.deploy.command)?;
            if site.deploy.command.trim().is_empty() {
                return Err("请填写发布命令（项目自带的发布脚本）".into());
            }
            site.deploy.command = site.deploy.command.trim().to_string();
        }
        "direct" | "pages_git" | "workers" => {
            let c = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "请选择 Cloudflare 连接".to_string())?;
            if c.kind != "cloudflare" {
                return Err("部署用的连接不是 Cloudflare".into());
            }
            let p = site.deploy.project.trim().to_ascii_lowercase();
            let ok_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || (site.deploy.mode == "workers" && c == '_');
            if p.is_empty() || !p.chars().all(ok_char) || p.starts_with('-') || p.len() > 63 {
                return Err(if site.deploy.mode == "workers" { "Worker 名称只能用小写字母、数字、- 和 _".into() } else { "Pages 项目名只能用小写字母、数字和连字符".into() });
            }
            site.deploy.project = p;
            if site.deploy.mode == "pages_git" && site.source != "github" && site_github_repo(&site).is_empty() {
                return Err("「Pages 绑定 GitHub」要求站点代码在 GitHub 上：本地文件夹需要有指向 GitHub 的 origin".into());
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
    if !(1..=5).contains(&site.ops.level) {
        return Err("自动化档位只能是 1–5".into());
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
        origin: String::new(),
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

/// 制作站点：建文件夹、写需求记录、git init、登记站点、存定位，
/// 返回站点和第一版对话的第一条消息（前端用它在这个站点里开对话）。
#[derive(Serialize)]
pub struct CreatedSite {
    pub site: Site,
    pub message: String,
}

#[tauri::command]
pub async fn create_site(state: tauri::State<'_, AppState>, req: crate::site_make::CreateSiteRequest) -> Result<CreatedSite, String> {
    use crate::site_make as mk;
    if req.name.trim().is_empty() {
        return Err("请填写站点名称".into());
    }
    if req.about.trim().is_empty() {
        return Err("请用一句话说清这是个什么站".into());
    }
    let build = mk::stack_build(&req.stack).ok_or("请选择技术栈")?;
    let parent = Path::new(req.parent_dir.trim());
    if !parent.is_absolute() || !parent.is_dir() {
        return Err("存放位置不存在".into());
    }
    let domain = sites::normalize_domain(&req.domain)?;
    let dir = parent.join(mk::folder_name(&domain, &req.name));
    if dir.exists() && std::fs::read_dir(&dir).map(|mut d| d.next().is_some()).unwrap_or(true) {
        return Err(format!("{} 已经存在且不是空文件夹，换个名字或位置", dir.display()));
    }
    let t = now();
    let mut site = Site {
        id: String::new(),
        name: req.name.trim().to_string(),
        domain,
        source: "local".into(),
        gh_conn_id: String::new(),
        repo: String::new(),
        local_dir: dir.to_string_lossy().into_owned(),
        default_branch: "main".into(),
        framework: req.stack.clone(),
        build,
        deploy: sites::DeployConfig { mode: "none".into(), ..Default::default() },
        data: sites::DataBinding::default(),
        ops: sites::OpsConfig::default(),
        pinned: false,
        tags: Vec::new(),
        origin: "created".into(),
        created_at: t,
        updated_at: t,
    };
    std::fs::create_dir_all(dir.join(".pilot")).map_err(|e| format!("创建文件夹失败：{e}"))?;
    std::fs::write(dir.join(".pilot/brief.md"), mk::brief_md(&req)).map_err(|e| e.to_string())?;
    let cleanup = |e: String| {
        let _ = std::fs::remove_dir_all(&dir);
        e
    };
    crate::launch_cmds::init_repo(&dir).await.map_err(cleanup)?;
    validate(&mut site, &state).map_err(cleanup)?;
    let site = state.sites.insert(site).map_err(cleanup)?;
    let _ = crate::site_stage::save_positioning(
        &state.sites.data_dir(&site.id),
        &crate::site_stage::Positioning { text: mk::positioning_text(&req).chars().take(1200).collect(), by: "user".into(), updated: t },
    );
    Ok(CreatedSite { message: mk::first_message(&req), site })
}

/// 站点制作的状态：站点约定写了没有、第一版进度、上线前检查（扫构建产物）。
#[derive(Serialize)]
pub struct MakeState {
    pub conventions: bool,
    /// AI 写在 .pilot/todo.json 里、我还没处理的事；None＝AI 还没写这个文件
    pub todos: Option<Vec<crate::site_make::Todo>>,
    /// .env.example 里声明的配置变量（「需要你处理」里对应变量名的输入直接存成配置变量）
    pub config: Vec<crate::site_config::ConfigVar>,
    pub steps: Vec<crate::site_make::MakeStep>,
    pub checks: Vec<crate::site_make::Check>,
    /// 构建产物的时间（index.html 的修改时间，秒）；0＝还没构建
    pub built_at: u64,
}

#[tauri::command]
pub async fn site_make_state(state: tauri::State<'_, AppState>, id: String, full: Option<bool>) -> Result<MakeState, String> {
    use crate::site_make as mk;
    let site = state.sites.get(&id)?;
    let gh = state.conns.get(&site.gh_conn_id).ok();
    let code = crate::site_git::code_dir(&site, gh.as_ref())?;
    let app = site.app_dir(&code);
    // GitHub 来源的「构建并检查」在 <站点数据>/deploy 工作树里构建（见 site_deploy::prepare_github），产物在那里
    let deploy = state.sites.data_dir(&id).join("deploy");
    let built_app = if site.source == "github" && deploy.is_dir() { site.app_dir(&deploy) } else { app.clone() };
    let out = mk::out_dir(&built_app, &site.build.out_dir);
    let commits = if full.unwrap_or(true) && code.join(".git").exists() {
        crate::site_git::git(&code, &["rev-list", "--count", "HEAD"], "").await.map(|o| o.stdout.trim().parse().unwrap_or(0)).unwrap_or(0)
    } else {
        0
    };
    let built_at = std::fs::metadata(out.join("index.html"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let todos = mk::load_todos(&code);
    let config = crate::site_config::vars(&site, &code);
    // 不要 full：只看站点约定和待办（站点看板每次打开都会问，别去扫整个构建产物）
    if !full.unwrap_or(true) {
        return Ok(MakeState { conventions: mk::has_conventions(&code), todos, config, steps: vec![], checks: vec![], built_at });
    }
    let (steps, mut checks) = tokio::task::spawn_blocking(move || (mk::progress(&app, &code, &built_app, &site.build.out_dir, commits), mk::readiness(&out)))
        .await
        .map_err(|e| e.to_string())?;
    if let Some(t) = &todos {
        if built_at > 0 {
            checks.push(mk::todo_check(t));
        }
    }
    Ok(MakeState { conventions: steps.iter().any(|s| s.key == "conventions" && s.done), todos, config, steps, checks, built_at })
}

/// 「继续制作」的消息：按当前进度列出还没完成的项，带上最新标准。
#[tauri::command]
pub async fn site_continue_message(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    use crate::site_make as mk;
    let site = state.sites.get(&id)?;
    let code = crate::site_git::code_dir(&site, state.conns.get(&site.gh_conn_id).ok().as_ref())?;
    let commits = crate::site_git::git(&code, &["rev-list", "--count", "HEAD"], "").await.map(|o| o.stdout.trim().parse().unwrap_or(0)).unwrap_or(0);
    let deploy = state.sites.data_dir(&id).join("deploy");
    let built_app = if site.source == "github" && deploy.is_dir() { site.app_dir(&deploy) } else { site.app_dir(&code) };
    let steps = mk::progress(&site.app_dir(&code), &code, &built_app, &site.build.out_dir, commits);
    let pending: Vec<&str> = steps.iter().filter(|s| !s.done).map(|s| s.label).collect();
    Ok(mk::continue_message(&pending, &site.framework))
}

/// 把文件夹移到废纸篓（可恢复），不直接删。
fn move_to_trash(p: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let url = NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|e| format!("移到废纸篓失败：{}", e.localizedDescription()))
    }
    #[cfg(target_os = "windows")]
    {
        let path = p.to_string_lossy().replace('\'', "''");
        let script = format!("Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('{path}', 'OnlyErrorDialogs', 'SendToRecycleBin')");
        let ok = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &script]).status().map(|s| s.success()).unwrap_or(false);
        if ok { Ok(()) } else { Err("移到回收站失败".into()) }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let ok = std::process::Command::new("gio").args(["trash", &p.to_string_lossy()]).status().map(|s| s.success()).unwrap_or(false);
        if ok { Ok(()) } else { Err("移到废纸篓失败（需要 gio）".into()) }
    }
}

/// 放弃一个在 Pilot 里制作的站点：文件夹移到废纸篓，再从 Pilot 移除。只对「在 Pilot 里制作的本地站点」开放——
/// 导入的站点是用户自己的代码，只能「移除」不能删文件夹。
#[tauri::command]
pub fn discard_made_site(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let site = state.sites.get(&id)?;
    if site.origin != "created" || site.source != "local" {
        return Err("只有在 Pilot 里制作的站点才能连文件夹一起删除".into());
    }
    if state.site_jobs.running(&id) {
        return Err("站点有任务正在运行，请先停止".into());
    }
    let conv = format!("site:{id}");
    if state.convos.list().iter().any(|c| c.conn_id == conv && c.status == "running") {
        return Err("站点下有对话正在运行，请先停止".into());
    }
    let dir = Path::new(&site.local_dir);
    if dir.is_dir() {
        move_to_trash(dir)?;
    }
    remove_site(state, id)
}

/// 让 AI 把需要我处理的事整理成 .pilot/todo.json 的消息（老一点的站点，AI 只在汇报里写了）。
#[tauri::command]
pub fn site_todo_request_message() -> String {
    format!("请把需要我处理的事（要我提供的信息、要我做的决定、你能做但要我点头的事）整理写进 `.pilot/todo.json`，只改这个文件，提交一次。\n\n{}", crate::site_make::TODO_SPEC)
}

/// 看板上把一条待办标成「已处理 / 不需要」：直接从 .pilot/todo.json 里删掉（AI 下次读到的就是剩下的）。
#[tauri::command]
pub fn site_todo_handled(state: tauri::State<'_, AppState>, id: String, todo_id: String) -> Result<(), String> {
    let site = state.sites.get(&id)?;
    let code = crate::site_git::code_dir(&site, state.conns.get(&site.gh_conn_id).ok().as_ref())?;
    crate::site_make::remove_todo(&code, &todo_id).map(|_| ())
}

/// 站点配置变量：.env.example 里声明的 + Pilot 里手动加的、哪些还没填、同步到哪、有没有改动没同步。不返回值。
#[tauri::command]
pub fn site_config_state(state: tauri::State<'_, AppState>, id: String) -> Result<crate::site_config::ConfigState, String> {
    let site = state.sites.get(&id)?;
    let code = crate::site_git::code_dir(&site, state.conns.get(&site.gh_conn_id).ok().as_ref())?;
    Ok(crate::site_config::state(&site, &code, &state.sites.data_dir(&id)))
}

/// 把配置变量同步到部署目标（Pages 环境变量 / Worker 密钥 / 服务器 env 文件）。
#[tauri::command]
pub async fn site_config_sync(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let code = crate::site_git::code_dir(&site, state.conns.get(&site.gh_conn_id).ok().as_ref())?;
    crate::site_config::sync(&site, &state.conns, &code, &state.sites.data_dir(&id)).await
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
    if !matches!(site.deploy.mode.as_str(), "direct" | "workers" | "vps") {
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
    if site.deploy.project.is_empty() || site.deploy.mode == "vps" {
        return Ok(vec![]);
    }
    let (token, acct) = cf_for(&state, &site)?;
    if site.deploy.mode == "workers" {
        return crate::cf::list_worker_deployments(&token, &acct, &site.deploy.project).await;
    }
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
// VPS 部署授权
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct VpsKeyStatus {
    pub has_key: bool,
    pub public_key: String,
    pub host: String,
}

fn vps_conn(state: &AppState, site: &Site) -> Result<crate::conns::Connection, String> {
    let c = state.conns.get(&site.deploy.ssh_conn_id).map_err(|_| "请先在站点设置里选择部署用的 SSH 连接并保存".to_string())?;
    if c.kind != "ssh" {
        return Err("部署连接不是 SSH 连接".into());
    }
    Ok(c)
}

#[tauri::command]
pub fn vps_key_status(state: tauri::State<'_, AppState>, id: String) -> Result<VpsKeyStatus, String> {
    let site = state.sites.get(&id)?;
    let c = vps_conn(&state, &site)?;
    Ok(VpsKeyStatus { has_key: crate::site_vps::has_key(&c.id), public_key: crate::site_vps::public_key(&c.id).unwrap_or_default(), host: format!("{}@{}", c.ssh_user, c.ssh_host) })
}

/// 为部署授权：生成 Pilot 专用部署密钥，通过这条 SSH 连接加进服务器的 authorized_keys，并实际登录验证一次。
#[tauri::command]
pub async fn setup_vps_key(state: tauri::State<'_, AppState>, id: String) -> Result<VpsKeyStatus, String> {
    let site = state.sites.get(&id)?;
    let c = vps_conn(&state, &site)?;
    let pk = crate::site_vps::install(&state.ssh, &c).await?;
    Ok(VpsKeyStatus { has_key: true, public_key: pk, host: format!("{}@{}", c.ssh_user, c.ssh_host) })
}

#[tauri::command]
pub async fn revoke_vps_key(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let site = state.sites.get(&id)?;
    let c = vps_conn(&state, &site)?;
    crate::site_vps::revoke(&state.ssh, &c).await
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
【站点信息】{root}框架 {framework}；构建 {build}，产物目录 `{out}`；上线前检查：{checks}。\n\
【上线】上线由 Pilot 负责：用户在站点页点「构建并上线」，或运营任务审核通过后，Pilot 会构建、跑检查、上传 Cloudflare 并提交 IndexNow。\
你不要自己部署——不要运行 `wrangler pages deploy`、`npm run deploy` 之类的命令，也拿不到 Cloudflare 凭据。\n\
【干活的规矩】仓库里有 `.pilot/site.md`（站点约定：内容放哪、页面格式、新增一页要改哪些地方）就先读它并照着做，改了目录结构或页面格式时同步更新它；\
再读 README、目录结构和相关页面，弄清现状再改；改动前用一两句话说明打算怎么改。\
改完先跑构建（和检查命令）确认通过再交付。面向访客的文字遵循随消息附带的公开文案规范。\
不要把流量、收入等运营数字写进代码、提交说明或 PR 描述（仓库可能是公开的）。{data}\n\
用用户所用语言回复，简洁说明做了什么、结果如何、还需要用户确认什么。",
        name = site.name,
        framework = if site.framework.is_empty() { "未知" } else { &site.framework },
        out = site.build.out_dir,
        root = if site.build.root.is_empty() { String::new() } else { format!("站点代码在仓库的 `{}/` 子目录里（构建、检查命令都在那里执行，页面和静态文件也在那里）；", site.build.root) },
    )
}

/// 全部站点的图标（data URL，没有就不出现在结果里）。各站并行查，本地文件优先，线上结果缓存 7 天。
#[tauri::command]
pub async fn site_icons(state: tauri::State<'_, AppState>) -> Result<std::collections::HashMap<String, String>, String> {
    let mut set = tokio::task::JoinSet::new();
    for site in state.sites.list() {
        let gh = state.conns.get(&site.gh_conn_id).ok();
        let code = crate::site_git::code_dir(&site, gh.as_ref()).ok().map(|c| site.app_dir(&c));
        let dir = state.sites.data_dir(&site.id);
        set.spawn(async move {
            let url = crate::site_icon::icon_for(&site, &dir, code).await;
            (site.id, url)
        });
    }
    let mut out = std::collections::HashMap::new();
    while let Some(r) = set.join_next().await {
        if let Ok((id, url)) = r {
            if !url.is_empty() {
                out.insert(id, url);
            }
        }
    }
    Ok(out)
}

#[derive(serde::Serialize)]
pub struct SiteStageView {
    #[serde(flatten)]
    pub stage: crate::site_stage::StageInfo,
    pub positioning: crate::site_stage::Positioning,
}

/// 站点阶段（自动判断 / 手动指定）和 AI 归纳的站点定位。
#[tauri::command]
pub fn site_stage_info(state: tauri::State<'_, AppState>, id: String) -> Result<SiteStageView, String> {
    let site = state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    Ok(SiteStageView { stage: crate::site_stage::stage_of(&site, &dir), positioning: crate::site_stage::load_positioning(&dir) })
}

/// 运营人员改写站点定位（之后 AI 照用、不覆盖）；传空串＝交还给 AI 下次重新归纳。
#[tauri::command]
pub fn save_site_positioning(state: tauri::State<'_, AppState>, id: String, text: String) -> Result<crate::site_stage::Positioning, String> {
    state.sites.get(&id)?;
    let dir = state.sites.data_dir(&id);
    let text = text.trim();
    if text.chars().count() > 1200 {
        return Err("站点定位太长了（1200 字以内）".into());
    }
    let p = if text.is_empty() {
        crate::site_stage::Positioning { by: "ai".into(), ..crate::site_stage::load_positioning(&dir) }
    } else {
        crate::site_stage::Positioning { text: text.into(), by: "user".into(), updated: crate::now_secs() }
    };
    crate::site_stage::save_positioning(&dir, &p)?;
    Ok(p)
}

#[cfg(test)]
mod cf_match_tests {
    use super::*;
    #[test]
    fn finds_pages_project_by_domain_or_repo() {
        let conn: crate::conns::Connection = serde_json::from_value(serde_json::json!({"id": "cf1", "name": "CF", "kind": "cloudflare", "skill_dir": "", "key_prefix": "", "key_kind": "cf_token", "account_id": "a", "login": "", "created_at": "1"})).unwrap();
        let pr = |name: &str, domains: &[&str], ty: &str, repo: &str| crate::cf::PagesProject { name: name.into(), domains: domains.iter().map(|d| d.to_string()).collect(), production_branch: "main".into(), source_type: ty.into(), source_repo: repo.into(), ..Default::default() };
        let all = vec![(conn, vec![pr("other", &["other.com"], "", ""), pr("xtexe", &["xtexe.pages.dev", "xtexe.com"], "github", "ccvar/xtexe.com")])];
        let mut site: crate::sites::Site = serde_json::from_value(serde_json::json!({"id": "s", "name": "s", "source": "local", "created_at": 0, "updated_at": 0})).unwrap();
        site.domain = "www.xtexe.com".into();
        site.repo = "ccvar/xtexe.com".into();
        let d = match_cf_deploy(&all, &site).unwrap();
        assert_eq!((d.project.as_str(), d.mode.as_str(), d.matched_by.as_str(), d.branch.as_str()), ("xtexe", "pages_git", "domain", "main"));
        // 仓库对不上：当直接上传型
        site.repo = "someone/else".into();
        assert_eq!(match_cf_deploy(&all, &site).unwrap().mode, "direct");
        // 域名对不上、仓库对得上
        site.domain = "new.example".into();
        site.repo = "ccvar/xtexe.com".into();
        assert_eq!(match_cf_deploy(&all, &site).unwrap().matched_by, "repo");
        site.repo = String::new();
        site.local_dir = "/nonexistent".into();
        assert!(match_cf_deploy(&all, &site).is_none());
    }
}

#[cfg(test)]
mod trash_tests {
    /// 真的会往废纸篓里放一个空文件夹，所以默认不跑：cargo test -- --ignored trash
    #[test]
    #[ignore]
    fn moves_a_folder_to_the_trash() {
        let d = std::env::temp_dir().join(format!("pilot-trash-check-{}", std::process::id()));
        std::fs::create_dir_all(d.join("x")).unwrap();
        super::move_to_trash(&d).unwrap();
        assert!(!d.exists());
    }
}
