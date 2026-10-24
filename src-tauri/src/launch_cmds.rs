//! 新站闭环：把在 Pilot 里做出来的站点（Cloudflare 建站项目）变成一个可运营的站点——
//! 导入为站点 → 代码放到 GitHub → 建 Pages 项目并绑定域名 → 建 GA4 媒体资源 → 添加并验证 Search Console → IndexNow。
//! 每一步都是用户在向导里点了才执行；需要管理权限的步骤只在 Google 连接是 OAuth 且授予了管理权限时可用。

use serde::Serialize;
use serde_json::json;
use std::path::Path;

use crate::sites::{self, Site};
use crate::AppState;

fn now() -> u64 {
    crate::now_secs()
}

/// 把 CF 连接下的建站项目导入为站点（本地来源，Pilot 直接上传）。已导入过则返回那个站点。
#[tauri::command]
pub async fn import_cf_project(state: tauri::State<'_, AppState>, conn_id: String, project: String) -> Result<Site, String> {
    let conn = state.conns.get(&conn_id)?;
    if conn.kind != "cloudflare" {
        return Err("这个连接不是 Cloudflare".into());
    }
    let slug = crate::cf_project_slug(&project);
    let dir = Path::new(&conn.skill_dir).join("projects").join(&slug);
    if !dir.is_dir() {
        return Err("项目目录不存在".into());
    }
    let dir = std::fs::canonicalize(&dir).map_err(|e| e.to_string())?;
    let dir_s = dir.to_string_lossy().into_owned();
    if let Some(s) = state.sites.list().into_iter().find(|s| s.local_dir == dir_s) {
        return Ok(s);
    }
    let d = dir.clone();
    let det = tauri::async_runtime::spawn_blocking(move || sites::detect(&d)).await.map_err(|e| e.to_string())?;
    // 建站项目通常还不是 git 仓库：初始化一次，后面的分支 / 回滚 / 推到 GitHub 都靠它。
    if !dir.join(".git").exists() {
        init_repo(&dir).await?;
    }
    let t = now();
    let site = Site {
        id: String::new(),
        name: if det.domain.is_empty() { slug.clone() } else { det.domain.clone() },
        domain: det.domain.clone(),
        source: "local".into(),
        gh_conn_id: String::new(),
        repo: String::new(),
        local_dir: dir_s,
        default_branch: "main".into(),
        framework: det.framework.clone(),
        build: det.build.clone(),
        deploy: sites::DeployConfig { mode: "direct".into(), cf_conn_id: conn.id.clone(), project: slug, branch: "main".into(), ..Default::default() },
        data: sites::DataBinding { indexnow_key: det.indexnow_key.clone(), ..Default::default() },
        ops: sites::OpsConfig::default(),
        pinned: false,
        tags: Vec::new(),
        origin: String::new(),
        created_at: t,
        updated_at: t,
    };
    state.sites.insert(site)
}

pub(crate) async fn init_repo(dir: &Path) -> Result<(), String> {
    use crate::site_git::git_ok;
    git_ok(dir, &["init", "-q", "-b", "main"], "").await?;
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "node_modules/\n.wrangler/\n.env\n.dev.vars\nuploads/\nshots/\n.DS_Store\n").map_err(|e| e.to_string())?;
    }
    git_ok(dir, &["add", "-A"], "").await?;
    let _ = crate::site_git::git_commit_ok(dir, &["commit", "-qm", "Initial commit"]).await;
    Ok(())
}

#[derive(Serialize, Default)]
pub struct LaunchStatus {
    /// 配置变量：声明了几个、哪些还没填、有没有改动没同步到部署目标
    pub config_vars: usize,
    pub config_missing: Vec<String>,
    pub config_needs_sync: bool,
    pub config_target: String,
    pub github: bool,
    pub repo: String,
    pub pages: bool,
    pub pages_domains: Vec<String>,
    pub domain_attached: bool,
    pub ga4: bool,
    pub gsc: bool,
    pub indexnow: bool,
    pub bing: bool,
    pub deployed: bool,
    /// 有没有一条授予了管理权限的 Google OAuth 连接（自动建 GA4 / 验证 Search Console 需要）。
    pub google_manage_conn: String,
    /// 这次检查时按域名自动绑定上的资源（「Search Console sc-domain:x.com」这类说明）。
    pub auto_bound: Vec<String>,
    pub errors: Vec<String>,
}

/// 向导的磁盘缓存（站点数据目录 `launch.json`）：重启后打开站点不用再等网络。
/// - `probed`：上次「按域名找资源」的时间，10 分钟最多一次（点刷新不受限）；
/// - `pages`：上次查到的 Cloudflare Pages 项目情况，快速读取时直接用（后台那次会刷新它）。
#[derive(Serialize, serde::Deserialize, Default, Clone)]
struct LaunchCache {
    #[serde(default)]
    probed: u64,
    #[serde(default)]
    pages: Option<PagesCache>,
}

#[derive(Serialize, serde::Deserialize, Default, Clone)]
struct PagesCache {
    project: String,
    exists: bool,
    domains: Vec<String>,
    at: u64,
}

static CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_cache(state: &AppState, id: &str) -> LaunchCache {
    std::fs::read(state.sites.data_dir(id).join("launch.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn update_cache(state: &AppState, id: &str, f: impl FnOnce(&mut LaunchCache)) {
    let _g = CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = read_cache(state, id);
    f(&mut c);
    let dir = state.sites.data_dir(id);
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(raw) = serde_json::to_vec(&c) {
        let _ = std::fs::write(dir.join("launch.json"), raw);
    }
}

/// 站点还没绑 Search Console / Bing 时，到已连接的账号里按域名找一遍，唯一命中就直接绑上。
/// 规则和「按域名绑定」一致；Google 只在站点已用的那个账号里找（一个站点的 GA4 和 Search Console 共用一条连接），
/// 站点还没选 Google 连接时才在所有账号里找。
async fn probe_bindings(state: &AppState, site: &Site, force: bool) -> (Vec<String>, Vec<String>) {
    let (mut bound, mut errs) = (Vec::new(), Vec::new());
    if site.domain.is_empty() || (!site.data.ga4_property.is_empty() && !site.data.gsc_site.is_empty() && !site.data.bing_site.is_empty()) {
        return (bound, errs);
    }
    let now = crate::now_secs();
    if !force && now.saturating_sub(read_cache(state, &site.id).probed) < 600 {
        return (bound, errs);
    }
    update_cache(state, &site.id, |c| c.probed = now);
    if site.data.ga4_property.is_empty() {
        // GA4：按媒体资源里网页数据流的网址匹配域名。站点已在用的账号先找；名字里带域名的媒体资源先查，命中即停。
        let mut conns: Vec<_> = state.conns.list().into_iter().filter(|c| c.kind == "google").collect();
        let first = site.data.google_conn_id.clone();
        conns.sort_by_key(|c| c.id != first);
        let label = site.domain.trim_start_matches("www.").split('.').next().unwrap_or_default().to_lowercase();
        'acct: for c in conns {
            let Ok(raw) = crate::keychain::get_key(&c.id) else { continue };
            let Ok(cred) = crate::google::parse_credential(&c.key_kind, &raw) else { continue };
            let token = match crate::google::access_token(&c.id, &cred, &crate::google::read_scopes()).await {
                Ok(t) => t,
                Err(e) => { errs.push(format!("GA4（{}）：{e}", if c.login.is_empty() { &c.name } else { &c.login })); continue }
            };
            let mut props = match crate::google::list_ga4_properties(&token).await {
                Ok(p) => p,
                Err(e) => { errs.push(format!("GA4（{}）：{e}", if c.login.is_empty() { &c.name } else { &c.login })); continue }
            };
            props.sort_by_key(|p| !p.name.to_lowercase().contains(&label));
            for p in props {
                let hosts = crate::google::ga4_stream_hosts(&token, &p.property).await.unwrap_or_default();
                if !hosts.iter().any(|h| crate::google::same_host(h, &site.domain)) { continue }
                let (cid, prop, name) = (c.id.clone(), p.property.clone(), p.name.clone());
                if state.sites.update(&site.id, crate::now_secs(), |x| {
                    // 换 GA4 账号时，原来跟着旧账号的 Search Console 改记成它自己的账号
                    if x.data.gsc_conn_id.is_empty() && !x.data.gsc_site.is_empty() && !x.data.google_conn_id.is_empty() && x.data.google_conn_id != cid {
                        x.data.gsc_conn_id = x.data.google_conn_id.clone();
                    }
                    x.data.google_conn_id = cid.clone();
                    x.data.ga4_property = prop;
                    x.data.ga4_property_name = name;
                    Ok(())
                }).is_ok() {
                    bound.push(format!("GA4 {}（{}）", p.name, if c.login.is_empty() { &c.name } else { &c.login }));
                }
                break 'acct;
            }
        }
    }
    // Search Console 那段要基于最新的绑定（上面可能刚改了 GA4 的账号）
    let site = &state.sites.get(&site.id).unwrap_or_else(|_| site.clone());
    if site.data.gsc_site.is_empty() {
        // 先找站点已经在用的账号，再找其它 Google 账号（Search Console 可以和 GA4 在不同账号下）
        let mut conns: Vec<_> = state.conns.list().into_iter().filter(|c| c.kind == "google").collect();
        let first = site.data.gsc_conn().to_string();
        conns.sort_by_key(|c| c.id != first);
        for c in conns {
            let Ok(raw) = crate::keychain::get_key(&c.id) else { continue };
            let Ok(cred) = crate::google::parse_credential(&c.key_kind, &raw) else { continue };
            let list = match crate::google::access_token(&c.id, &cred, &crate::google::read_scopes()).await {
                Ok(t) => crate::google::list_gsc_sites(&t).await,
                Err(e) => Err(e),
            };
            match list {
                Ok(list) => {
                    if let Some(g) = crate::google::match_gsc_site(&list, &site.domain) {
                        let (cid, url) = (c.id.clone(), g.url.clone());
                        if state.sites.update(&site.id, crate::now_secs(), |x| {
                            if x.data.google_conn_id.is_empty() { x.data.google_conn_id = cid.clone(); }
                            x.data.gsc_conn_id = if x.data.google_conn_id == cid { String::new() } else { cid };
                            x.data.gsc_site = url;
                            Ok(())
                        }).is_ok() {
                            bound.push(format!("Search Console {}（{}）", g.url, if c.login.is_empty() { c.name.clone() } else { c.login.clone() }));
                        }
                        break;
                    }
                }
                Err(e) => errs.push(format!("Search Console（{}）：{e}", c.remark.trim().is_empty().then(|| c.name.clone()).unwrap_or_else(|| c.remark.clone()))),
            }
        }
    }
    if site.data.bing_site.is_empty() {
        for c in state.conns.list().into_iter().filter(|c| c.kind == "bing") {
            let Ok(key) = crate::keychain::get_key(&c.id) else { continue };
            match crate::bing::list_sites(&key).await {
                Ok(list) => {
                    let hit = list.iter().find(|b| crate::sites::normalize_domain(&b.url).is_ok_and(|h| crate::google::same_host(&h, &site.domain)));
                    if let Some(b) = hit {
                        let (cid, url) = (c.id.clone(), b.url.clone());
                        if state.sites.update(&site.id, crate::now_secs(), |x| {
                            x.data.bing_conn_id = cid;
                            x.data.bing_site = url;
                            Ok(())
                        }).is_ok() {
                            bound.push(format!("Bing {}", b.url));
                        }
                        break;
                    }
                }
                Err(e) => errs.push(format!("Bing：{e}")),
            }
        }
    }
    (bound, errs)
}

#[tauri::command]
/// `probe=false` 只读当前状态（快，打开页面先用它）；默认会按域名去已连接账号里找资源（较慢，有 10 分钟节流）；
/// `force=true` 是手动刷新，不受节流。
pub async fn launch_status(state: tauri::State<'_, AppState>, id: String, force: Option<bool>, probe: Option<bool>) -> Result<LaunchStatus, String> {
    let site = state.sites.get(&id)?;
    let force = force.unwrap_or(false);
    let quick = !force && !probe.unwrap_or(true);
    let (auto_bound, probe_errs) = if force || probe.unwrap_or(true) { probe_bindings(&state, &site, force).await } else { (Vec::new(), Vec::new()) };
    let site = if auto_bound.is_empty() { site } else { state.sites.get(&id)? };
    let mut st = LaunchStatus {
        github: site.source == "github" || !site.repo.is_empty(),
        repo: site.repo.clone(),
        ga4: !site.data.ga4_property.is_empty(),
        gsc: !site.data.gsc_site.is_empty(),
        indexnow: !site.data.indexnow_key.is_empty(),
        bing: !site.data.bing_site.is_empty(),
        deployed: crate::site_deploy::load_history(&state.sites.data_dir(&id)).iter().any(|r| r.ok),
        auto_bound,
        errors: probe_errs,
        ..Default::default()
    };
    if matches!(site.deploy.mode.as_str(), "workers" | "vps") {
        // Workers / VPS 的路由、域名由项目自己的配置管，不走 Pages 这一步。 / 自定义域名在仓库的 wrangler 配置里管理，不走 Pages 这一步。
        st.pages = true;
        st.domain_attached = true;
    } else if let Some(pc) = read_cache(&state, &id).pages.filter(|p| quick && p.project == site.deploy.project && crate::now_secs().saturating_sub(p.at) < 86_400) {
        // 快速读取：用上次查到的结果，后台那次再去 Cloudflare 刷新
        st.pages = pc.exists;
        st.domain_attached = !site.domain.is_empty() && pc.domains.iter().any(|d| d == &site.domain);
        st.pages_domains = pc.domains;
    } else if let Ok(cf) = state.conns.get(&site.deploy.cf_conn_id) {
        if let Ok(token) = crate::keychain::get_key(&cf.id) {
            match crate::cf::get_pages_project(&token, &cf.account_id, &site.deploy.project).await {
                Ok(p) => {
                    let (exists, domains) = p.map(|p| (true, p.domains)).unwrap_or((false, Vec::new()));
                    st.pages = exists;
                    st.domain_attached = !site.domain.is_empty() && domains.iter().any(|d| d == &site.domain);
                    st.pages_domains = domains.clone();
                    let project = site.deploy.project.clone();
                    update_cache(&state, &id, |c| c.pages = Some(PagesCache { project, exists, domains, at: crate::now_secs() }));
                }
                Err(e) => st.errors.push(format!("Cloudflare：{e}")),
            }
        }
    }
    for c in state.conns.list().into_iter().filter(|c| c.kind == "google" && c.key_kind == "oauth") {
        if let Ok(raw) = crate::keychain::get_key(&c.id) {
            if let Ok(crate::google::Credential::OAuth(o)) = crate::google::parse_credential("oauth", &raw) {
                if crate::google_oauth::can_manage(&o) {
                    st.google_manage_conn = c.id.clone();
                    break;
                }
            }
        }
    }
    if let Ok(code) = crate::site_git::code_dir(&site, state.conns.get(&site.gh_conn_id).ok().as_ref()) {
        let cs = crate::site_config::state(&site, &code, &state.sites.data_dir(&id));
        st.config_vars = cs.vars.len();
        st.config_missing = cs.missing;
        st.config_needs_sync = cs.needs_sync;
        st.config_target = cs.target_label;
    }
    Ok(st)
}

/// 把站点代码放到 GitHub：新建仓库（Token 有账号级 Administration 权限时）或用一个已有的空仓库，
/// 推送当前代码，然后把站点的代码来源切换为 GitHub（之后的运营改动走分支 + PR）。
#[tauri::command]
pub async fn site_to_github(
    state: tauri::State<'_, AppState>,
    id: String,
    gh_conn_id: String,
    repo: String,
    create: bool,
    private: bool,
) -> Result<Site, String> {
    let site = state.sites.get(&id)?;
    if site.source == "github" {
        return Err("这个站点已经在用 GitHub 管理代码".into());
    }
    let gh = state.conns.get(&gh_conn_id)?;
    if gh.kind != "github" {
        return Err("请选择 GitHub 连接".into());
    }
    let token = crate::keychain::get_key(&gh.id)?;
    let full = if create {
        let name = repo.trim().rsplit('/').next().unwrap_or("").to_string();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
            return Err("仓库名只能用字母、数字、- _ .".into());
        }
        crate::site_github::create_repo(&token, &name, private, &format!("{}（由 Cligc Pilot 创建）", site.name))
            .await
            .map_err(|e| format!("{e}\n\n这枚 fine-grained Token 可能没有建仓库的权限（需要「All repositories」+ Administration 写权限）。可以先在 GitHub 上建一个空仓库，并在 Token 里授权它，再选「用已有的空仓库」。"))?
    } else {
        crate::github::parse_repo(repo.trim())?;
        repo.trim().to_string()
    };
    if !crate::site_github::repo_is_empty(&token, &full).await? {
        return Err(format!("{full} 不是空仓库。为避免覆盖已有代码，请换一个空仓库"));
    }
    let dir = std::path::PathBuf::from(&site.local_dir);
    if !dir.join(".git").exists() {
        init_repo(&dir).await?;
    }
    if !crate::site_git::is_clean(&dir).await {
        crate::site_git::git_ok(&dir, &["add", "-A"], "").await?;
        let _ = crate::site_git::git_commit_ok(&dir, &["commit", "-qm", "Update site"]).await;
    }
    let branch = crate::site_git::current_branch(&dir).await;
    let branch = if branch.is_empty() { "main".to_string() } else { branch };
    let url = format!("https://github.com/{full}.git");
    // 只推送；远程地址里不带凭据，凭据走 env 里的助手。
    crate::site_git::git_ok(&dir, &["push", &url, &format!("HEAD:refs/heads/{branch}")], &token).await?;
    let _ = crate::site_git::git(&dir, &["remote", "add", "origin", &url], "").await;
    // 克隆进 GitHub 连接的工作区，之后站点会话和运营任务都在那份检出里。
    let checkout = crate::github::repo_dir(&gh.skill_dir, &full)?;
    crate::github::ensure_checkout(&gh.skill_dir, &checkout.to_string_lossy(), &token, &gh.login, &gh.account_id).await?;
    // 构建变量不变；原来的建站项目文件夹保留，只是站点不再指向它。
    state.sites.update(&id, now(), |s| {
        s.source = "github".into();
        s.gh_conn_id = gh.id.clone();
        s.repo = full.clone();
        s.default_branch = branch.clone();
        s.local_dir = String::new();
        if s.deploy.branch.is_empty() {
            s.deploy.branch = branch.clone();
        }
        Ok(())
    })
}

#[derive(Serialize)]
pub struct PagesSetup {
    pub created: bool,
    pub domain_added: bool,
    pub dns: String,
}

/// 建 Pages 项目（直接上传型）、加自定义域名，并在同账号的 Zone 里补一条指向 pages.dev 的 CNAME。
#[tauri::command]
pub async fn site_setup_pages(state: tauri::State<'_, AppState>, id: String) -> Result<PagesSetup, String> {
    let site = state.sites.get(&id)?;
    let cf = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "请先在站点设置里选择 Cloudflare 连接".to_string())?;
    let token = crate::keychain::get_key(&cf.id)?;
    if site.deploy.project.is_empty() {
        return Err("请先填写 Pages 项目名".into());
    }
    let branch = if site.deploy.branch.is_empty() { "main" } else { &site.deploy.branch };
    let existing = crate::cf::get_pages_project(&token, &cf.account_id, &site.deploy.project).await?;
    let created = existing.is_none();
    let project = match existing {
        Some(p) => p,
        None => crate::cf::create_pages_project(&token, &cf.account_id, &site.deploy.project, branch).await?,
    };
    let mut out = PagesSetup { created, domain_added: false, dns: String::new() };
    if site.domain.is_empty() {
        out.dns = "站点没有填写域名，跳过绑定".into();
        return Ok(out);
    }
    crate::cf::add_pages_domain(&token, &cf.account_id, &site.deploy.project, &site.domain).await?;
    out.domain_added = true;
    let target = if project.subdomain.is_empty() { format!("{}.pages.dev", site.deploy.project) } else { project.subdomain.clone() };
    out.dns = match crate::cf::zone_for(&token, &site.domain).await {
        Ok(Some(zone)) => match crate::cf::ensure_dns_record(&token, &zone.id, "CNAME", &site.domain, &target, true).await {
            Ok(true) => format!("已在 {} 添加 CNAME → {target}", zone.name),
            Ok(false) => "域名已有 DNS 记录，未改动（如果不是指向 Pages，请手动调整）".into(),
            Err(e) => format!("自动添加 DNS 失败（{e}），请在 Cloudflare 里把 {} 的 CNAME 指向 {target}", site.domain),
        },
        Ok(None) => format!("域名不在这个 Cloudflare 账号里：请在域名服务商处把 {} 的 CNAME 指向 {target}", site.domain),
        Err(e) => format!("读取 Zone 失败（{e}），请手动把 CNAME 指向 {target}"),
    };
    Ok(out)
}

async fn manage_token(state: &AppState, conn_id: &str) -> Result<String, String> {
    let c = state.conns.get(conn_id)?;
    let raw = crate::keychain::get_key(&c.id)?;
    let cred = crate::google::parse_credential(&c.key_kind, &raw)?;
    match &cred {
        crate::google::Credential::OAuth(o) if crate::google_oauth::can_manage(o) => {}
        _ => return Err("需要一条授予了管理权限的 Google 账号登录（OAuth）连接".into()),
    }
    crate::google::access_token(
        &c.id,
        &cred,
        &[crate::google::SCOPE_ANALYTICS_EDIT, crate::google::SCOPE_WEBMASTERS, crate::google::SCOPE_SITE_VERIFICATION, crate::google::SCOPE_ANALYTICS_RO, crate::google::SCOPE_WEBMASTERS_RO],
    )
    .await
}

#[derive(Serialize)]
pub struct Ga4Created {
    pub property: String,
    pub measurement_id: String,
}

/// 在 Google 账号下建 GA4 媒体资源 + 网页数据流，绑定到站点，返回衡量 ID（用来在站点里埋代码）。
#[tauri::command]
pub async fn ga4_create_property(state: tauri::State<'_, AppState>, id: String, conn_id: String, account: String) -> Result<Ga4Created, String> {
    create_ga4(&state, &id, &conn_id, account).await
}

async fn create_ga4(state: &AppState, id: &str, conn_id: &str, account: String) -> Result<Ga4Created, String> {
    let id = id.to_string();
    let conn_id = conn_id.to_string();
    let site = state.sites.get(&id)?;
    if site.domain.is_empty() {
        return Err("请先填写站点域名".into());
    }
    let token = manage_token(state, &conn_id).await?;
    let account = if account.is_empty() {
        let v = crate::google::api(&token, reqwest::Method::GET, "https://analyticsadmin.googleapis.com/v1beta/accountSummaries?pageSize=50", None).await?;
        v["accountSummaries"].as_array().and_then(|a| a.first()).and_then(|a| a["account"].as_str()).map(str::to_string).ok_or("这个 Google 账号下没有 GA 账号，请先在 analytics.google.com 建一个")?
    } else {
        account
    };
    let prop = crate::google::api(
        &token,
        reqwest::Method::POST,
        "https://analyticsadmin.googleapis.com/v1beta/properties",
        Some(json!({ "parent": account, "displayName": site.domain, "timeZone": "Asia/Shanghai", "currencyCode": "USD" })),
    )
    .await?;
    let property = prop["name"].as_str().ok_or("Google 没有返回媒体资源 id")?.to_string();
    let stream = crate::google::api(
        &token,
        reqwest::Method::POST,
        &format!("https://analyticsadmin.googleapis.com/v1beta/{property}/dataStreams"),
        Some(json!({ "type": "WEB_DATA_STREAM", "displayName": site.domain, "webStreamData": { "defaultUri": format!("https://{}", site.domain) } })),
    )
    .await?;
    let measurement_id = stream["webStreamData"]["measurementId"].as_str().unwrap_or_default().to_string();
    let p = property.clone();
    state.sites.update(&id, now(), |s| {
        s.data.google_conn_id = conn_id.clone();
        s.data.ga4_property = p.clone();
        s.data.ga4_property_name = s.domain.clone();
        Ok(())
    })?;
    Ok(Ga4Created { property, measurement_id })
}

#[derive(Serialize)]
pub struct Ga4Config {
    pub measurement_id: String,
    /// 存进了哪个配置变量
    pub key: String,
    /// 这个变量在 .env.example 里声明过（没声明＝老站点，还要让 AI 按配置变量的规矩接上）
    pub declared: bool,
    pub created: bool,
}

/// 把 GA4 接进站点的配置变量：给了衡量 ID 就用它；站点已绑 GA4 媒体资源就去 Google 读它的衡量 ID；
/// 都没有就用 conn_id 这条 Google 账号新建一个。拿到后存进配置变量（.env.example 里声明的 GA 变量，没有就用 PUBLIC_GA_ID）。
#[tauri::command]
pub async fn ga4_to_config(state: tauri::State<'_, AppState>, id: String, conn_id: Option<String>, measurement_id: Option<String>) -> Result<Ga4Config, String> {
    let site = state.sites.get(&id)?;
    let mut created = false;
    let mid = if let Some(m) = measurement_id.map(|m| m.trim().to_uppercase()).filter(|m| !m.is_empty()) {
        if !m.starts_with("G-") || m.len() < 6 || !m[2..].chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err("衡量 ID 的格式是 G- 开头，比如 G-AB12CD34EF".into());
        }
        m
    } else if !site.data.ga4_property.is_empty() {
        ga4_stream_id(&state, &site).await?
    } else {
        let conn = conn_id.filter(|c| !c.is_empty()).ok_or("还没有能建 GA4 的 Google 账号：先在连接里用浏览器登录一个 Google 账号，或直接填已有的衡量 ID")?;
        created = true;
        create_ga4(&state, &id, &conn, String::new()).await?.measurement_id
    };
    if mid.is_empty() {
        return Err("Google 没有返回衡量 ID：到 GA4 后台看一下这个媒体资源的网页数据流".into());
    }
    let site = state.sites.get(&id)?;
    let code = crate::site_git::code_dir(&site, state.conns.get(&site.gh_conn_id).ok().as_ref())?;
    let vars = crate::site_config::vars(&site, &code);
    let (key, declared) = match crate::site_config::ga_key(&vars) {
        Some(k) => (k, true),
        None => ("PUBLIC_GA_ID".to_string(), false),
    };
    let keys = crate::sites::save_env(&id, &[(key.clone(), mid.clone())], &[])?;
    state.sites.update(&id, now(), |s| {
        s.build.env_keys = keys;
        Ok(())
    })?;
    Ok(Ga4Config { measurement_id: mid, key, declared, created })
}

/// 已绑定的 GA4 媒体资源的网页数据流衡量 ID（用站点数据那条 Google 连接，只读权限就够）。
async fn ga4_stream_id(state: &AppState, site: &crate::sites::Site) -> Result<String, String> {
    let c = state.conns.get(&site.data.google_conn_id).map_err(|_| "站点绑定 GA4 用的 Google 连接不在了".to_string())?;
    let raw = crate::keychain::get_key(&c.id)?;
    let cred = crate::google::parse_credential(&c.key_kind, &raw)?;
    let token = crate::google::access_token(&c.id, &cred, &[crate::google::SCOPE_ANALYTICS_RO]).await?;
    let v = crate::google::api(&token, reqwest::Method::GET, &format!("https://analyticsadmin.googleapis.com/v1beta/{}/dataStreams", site.data.ga4_property), None).await?;
    Ok(v["dataStreams"].as_array().into_iter().flatten().find_map(|s| s["webStreamData"]["measurementId"].as_str()).unwrap_or_default().to_string())
}

/// Search Console：取 DNS 验证令牌 → 在 Cloudflare 的 Zone 里写 TXT → 验证所有权 → 添加网域资源 → 提交 sitemap。
#[tauri::command]
pub async fn gsc_setup_domain(state: tauri::State<'_, AppState>, id: String, conn_id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let domain = site.domain.trim_start_matches("www.").to_string();
    if domain.is_empty() {
        return Err("请先填写站点域名".into());
    }
    let token = manage_token(&state, &conn_id).await?;
    let ident = json!({ "type": "INET_DOMAIN", "identifier": domain });
    let t = crate::google::api(
        &token,
        reqwest::Method::POST,
        "https://www.googleapis.com/siteVerification/v1/token",
        Some(json!({ "site": ident, "verificationMethod": "DNS_TXT" })),
    )
    .await?;
    let txt = t["token"].as_str().ok_or("Google 没有返回验证令牌")?.to_string();
    let cf = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "需要站点的 Cloudflare 连接来写 DNS 验证记录".to_string())?;
    let cf_token = crate::keychain::get_key(&cf.id)?;
    let zone = crate::cf::zone_for(&cf_token, &domain).await?.ok_or_else(|| format!("{domain} 不在这个 Cloudflare 账号里，无法自动写 TXT。请手动添加 TXT 记录：{txt}"))?;
    crate::cf::ensure_dns_record(&cf_token, &zone.id, "TXT", &domain, &txt, false).await?;
    // DNS 生效需要一点时间：最多重试 6 次，每次间隔 15 秒。
    let mut last = String::new();
    let mut verified = false;
    for _ in 0..6 {
        match crate::google::api(
            &token,
            reqwest::Method::POST,
            "https://www.googleapis.com/siteVerification/v1/webResource?verificationMethod=DNS_TXT",
            Some(json!({ "site": ident })),
        )
        .await
        {
            Ok(_) => {
                verified = true;
                break;
            }
            Err(e) => last = e,
        }
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    }
    if !verified {
        return Err(format!("TXT 记录已写入，但 Google 还没验证通过（{last}）。DNS 生效可能要几分钟，稍后再点一次即可"));
    }
    let site_url = format!("sc-domain:{domain}");
    crate::google::api(&token, reqwest::Method::PUT, &format!("https://www.googleapis.com/webmasters/v3/sites/{}", crate::google::encode_site(&site_url)), None).await?;
    // sitemap：构建产物或站点根里有哪个就交哪个
    let mut sitemap_note = String::new();
    for name in ["sitemap-index.xml", "sitemap.xml"] {
        let url = format!("https://{}/{name}", site.domain);
        let ok = reqwest::Client::new().head(&url).timeout(std::time::Duration::from_secs(15)).send().await.map(|r| r.status().is_success()).unwrap_or(false);
        if ok {
            match crate::google::gsc_submit_sitemap(&token, &site_url, &url).await {
                Ok(_) => sitemap_note = format!("，已提交 {name}"),
                Err(e) => sitemap_note = format!("，提交 sitemap 失败：{e}"),
            }
            break;
        }
    }
    let u = site_url.clone();
    state.sites.update(&id, now(), |s| {
        // 只改 Search Console 这一侧，不动 GA4 用的账号
        if s.data.google_conn_id.is_empty() { s.data.google_conn_id = conn_id.clone(); }
        s.data.gsc_conn_id = if s.data.google_conn_id == conn_id { String::new() } else { conn_id.clone() };
        s.data.gsc_site = u.clone();
        Ok(())
    })?;
    Ok(format!("已验证并添加 {site_url}{sitemap_note}"))
}

/// 列出 Google 账号下的 GA 账号（建媒体资源时选挂在哪个账号下）。
#[tauri::command]
pub async fn ga4_accounts(state: tauri::State<'_, AppState>, conn_id: String) -> Result<Vec<(String, String)>, String> {
    let token = manage_token(&state, &conn_id).await?;
    let v = crate::google::api(&token, reqwest::Method::GET, "https://analyticsadmin.googleapis.com/v1beta/accountSummaries?pageSize=50", None).await?;
    Ok(v["accountSummaries"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| Some((x["account"].as_str()?.to_string(), x["displayName"].as_str().unwrap_or("").to_string()))).collect())
        .unwrap_or_default())
}
