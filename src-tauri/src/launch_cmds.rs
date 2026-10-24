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
    /// 域名加到了 Pages 项目、但 Cloudflare 还在验证（DNS 生效前）：pending 等；生效了或没加过为空
    pub domain_status: String,
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
    /// Pilot 已把站点加到 Bing、还在等验证
    pub bing_pending: bool,
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
    #[serde(default)]
    pending: String,
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
        let bings: Vec<_> = state.conns.list().into_iter().filter(|c| c.kind == "bing").collect();
        let mut found = false;
        for c in &bings {
            let Ok(key) = crate::keychain::get_key(&c.id) else { continue };
            match crate::bing::list_sites(&key).await {
                Ok(list) => {
                    let hit = list.iter().find(|b| crate::sites::normalize_domain(&b.url).is_ok_and(|h| crate::google::same_host(&h, &site.domain)));
                    if let Some(b) = hit {
                        found = true;
                        // 加过但还没验证（多半是 Pilot 上次加的）：接着验证，过了才绑
                        if !b.verified {
                            match bing_verify(state, site, &c.id, &key, b).await {
                                Ok(true) => bound.push(format!("Bing {}（已验证）", b.url)),
                                Ok(false) => {}
                                Err(e) => errs.push(format!("Bing：{e}")),
                            }
                            break;
                        }
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
        // 哪个 Bing 账号里都没有：用第一条 Bing 连接直接加进去，再自动验证（DNS 记录 + 上线时放验证文件）
        if !found {
            if let Some(c) = bings.first() {
                if let Ok(key) = crate::keychain::get_key(&c.id) {
                    let url = format!("https://{}/", site.domain);
                    let added = crate::bing::add_site(&key, &url).await;
                    match crate::bing::list_sites(&key).await.map(|l| l.into_iter().find(|b| crate::sites::normalize_domain(&b.url).is_ok_and(|h| crate::google::same_host(&h, &site.domain)))) {
                        Ok(Some(b)) => match bing_verify(state, site, &c.id, &key, &b).await {
                            Ok(true) => bound.push(format!("Bing {}（已加入并验证）", b.url)),
                            Ok(false) => bound.push(format!("已把 {} 加到 Bing，等验证通过后自动绑定", site.domain)),
                            Err(e) => errs.push(format!("Bing：{e}")),
                        },
                        Ok(None) => errs.push(format!("Bing：没能把 {} 加进去{}", site.domain, added.err().map(|e| format!("（{e}）")).unwrap_or_default())),
                        Err(e) => errs.push(format!("Bing：{e}")),
                    }
                }
            }
        }
    }
    (bound, errs)
}

/// 自动验证 Bing 里的站点：域名在已连接的 Cloudflare 里就加一条 DNS 验证记录（`<码>.<域名>` CNAME → verify.bing.com），
/// 验证码记进站点（Pilot 上线时把 BingSiteAuth.xml 放进构建产物），然后让 Bing 验证；通过就绑上。
async fn bing_verify(state: &AppState, site: &Site, conn_id: &str, key: &str, b: &crate::bing::BingSite) -> Result<bool, String> {
    if !b.auth_code.is_empty() && site.data.bing_auth != b.auth_code {
        let code = b.auth_code.clone();
        let _ = state.sites.update(&site.id, crate::now_secs(), |x| {
            x.data.bing_auth = code;
            Ok(())
        });
    }
    if !b.dns_code.is_empty() {
        let mut conns: Vec<_> = state.conns.list().into_iter().filter(|c| c.kind == "cloudflare").collect();
        conns.sort_by_key(|c| c.id != site.deploy.cf_conn_id);
        for c in conns {
            let Ok(token) = crate::keychain::get_key(&c.id) else { continue };
            if let Ok(Some(zone)) = crate::cf::zone_for(&token, &site.domain).await {
                let name = format!("{}.{}", b.dns_code.to_ascii_lowercase(), site.domain);
                let _ = crate::cf::ensure_dns_record(&token, &zone.id, "CNAME", &name, "verify.bing.com", false).await;
                break;
            }
        }
    }
    let ok = crate::bing::verify_site(key, &b.url).await.unwrap_or(false);
    if ok {
        let (cid, url) = (conn_id.to_string(), b.url.clone());
        state.sites.update(&site.id, crate::now_secs(), |x| {
            x.data.bing_conn_id = cid;
            x.data.bing_site = url;
            Ok(())
        })?;
    }
    Ok(ok)
}

#[tauri::command]
/// `probe=false` 只读当前状态（快，打开页面先用它）；默认会按域名去已连接账号里找资源（较慢，有 10 分钟节流）；
/// `force=true` 是手动刷新，不受节流。
pub async fn launch_status(state: tauri::State<'_, AppState>, id: String, force: Option<bool>, probe: Option<bool>) -> Result<LaunchStatus, String> {
    let site = state.sites.get(&id)?;
    let force = force.unwrap_or(false);
    let quick = !force && !probe.unwrap_or(true);
    let (auto_bound, probe_errs) = if force || probe.unwrap_or(true) { probe_bindings(&state, &site, force).await } else { (Vec::new(), Vec::new()) };
    let mut site = if auto_bound.is_empty() { site } else { state.sites.get(&id)? };
    // IndexNow 不用点：Pilot 构建上传的站点没有密钥就直接生成（只是一串随机字符），验证文件随下次上线放进构建产物
    if site.data.indexnow_key.is_empty() && !site.domain.is_empty() && matches!(site.deploy.mode.as_str(), "direct" | "workers") {
        let key = crate::bing::new_indexnow_key();
        site = state.sites.update(&id, now(), |s| {
            s.data.indexnow_key = key.clone();
            Ok(())
        })?;
    }
    let mut st = LaunchStatus {
        github: site.source == "github" || !site.repo.is_empty(),
        repo: site.repo.clone(),
        ga4: !site.data.ga4_property.is_empty(),
        gsc: !site.data.gsc_site.is_empty(),
        indexnow: !site.data.indexnow_key.is_empty(),
        bing: !site.data.bing_site.is_empty(),
        bing_pending: site.data.bing_site.is_empty() && !site.data.bing_auth.is_empty(),
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
        st.domain_attached = !site.domain.is_empty() && (pc.domains.iter().any(|d| d == &site.domain) || !pc.pending.is_empty());
        st.domain_status = pc.pending.clone();
        st.pages_domains = pc.domains;
    } else if let Ok(cf) = state.conns.get(&site.deploy.cf_conn_id) {
        if let Ok(token) = crate::keychain::get_key(&cf.id) {
            match crate::cf::get_pages_project(&token, &cf.account_id, &site.deploy.project).await {
                Ok(p) => {
                    let (exists, domains) = p.map(|p| (true, p.domains)).unwrap_or((false, Vec::new()));
                    st.pages = exists;
                    let live = !site.domain.is_empty() && domains.iter().any(|d| d == &site.domain);
                    // 还没生效：看看是不是已经加了、正在等 Cloudflare 验证（DNS 生效后几分钟）
                    let pending = if exists && !live && !site.domain.is_empty() {
                        crate::cf::pages_domain_status(&token, &cf.account_id, &site.deploy.project, &site.domain).await.ok().flatten().filter(|s| s != "active").unwrap_or_default()
                    } else {
                        String::new()
                    };
                    st.domain_attached = live || !pending.is_empty();
                    st.domain_status = pending.clone();
                    st.pages_domains = domains.clone();
                    let project = site.deploy.project.clone();
                    update_cache(&state, &id, |c| c.pages = Some(PagesCache { project, exists, domains, pending, at: crate::now_secs() }));
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
    /// 域名下已有、指向别处的记录（「类型 内容」）：要你确认后才替换成指向 Pages
    pub conflict: Vec<String>,
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
    let mut out = PagesSetup { created, domain_added: false, dns: String::new(), conflict: vec![] };
    if site.domain.is_empty() {
        out.dns = "站点没有填写域名，跳过绑定".into();
        return Ok(out);
    }
    crate::cf::add_pages_domain(&token, &cf.account_id, &site.deploy.project, &site.domain).await?;
    out.domain_added = true;
    let target = if project.subdomain.is_empty() { format!("{}.pages.dev", site.deploy.project) } else { project.subdomain.clone() };
    match crate::cf::zone_for(&token, &site.domain).await {
        Ok(Some(zone)) => match crate::cf::dns_records(&token, &zone.id, &site.domain).await {
            Ok(recs) if recs.is_empty() => {
                out.dns = match crate::cf::ensure_dns_record(&token, &zone.id, "CNAME", &site.domain, &target, true).await {
                    Ok(_) => format!("已在 {} 添加 CNAME → {target}", zone.name),
                    Err(e) => format!("自动添加 DNS 失败（{e}），请在 Cloudflare 里把 {} 的 CNAME 指向 {target}", site.domain),
                };
            }
            Ok(recs) if recs.iter().any(|r| r.kind == "CNAME" && r.content.eq_ignore_ascii_case(&target)) => {
                out.dns = format!("{} 已经指向 {target}", site.domain);
            }
            Ok(recs) => {
                out.conflict = recs.iter().map(|r| format!("{} {}{}", r.kind, r.content, if r.proxied { "（已代理）" } else { "" })).collect();
                out.dns = format!("{} 已经有 DNS 记录（{}），指向的不是 Pages，没有改动。要换成指向 {target}，点「替换成指向 Pages」", site.domain, out.conflict.join("、"));
            }
            Err(e) => out.dns = format!("读取 DNS 记录失败（{e}），请在 Cloudflare 里把 {} 的 CNAME 指向 {target}", site.domain),
        },
        Ok(None) => out.dns = format!("在这个 Cloudflare 账号里没找到 {} 的 DNS（域名在别的账号或别的服务商，或者令牌没有「区域 → DNS → 编辑」权限）：把 {} 的 CNAME 指向 {target}，或给令牌补上权限后再点一次", site.domain, site.domain),
        Err(e) => out.dns = format!("读取 Zone 失败（{e}），请手动把 CNAME 指向 {target}"),
    };
    Ok(out)
}

/// 把域名现有的 A / AAAA / CNAME 记录换成指向 Pages 的 CNAME（界面上确认过才调用）。
#[tauri::command]
pub async fn site_pages_dns_replace(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    if site.domain.is_empty() {
        return Err("站点没有填写域名".into());
    }
    let cf = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "请先在站点设置里选择 Cloudflare 连接".to_string())?;
    let token = crate::keychain::get_key(&cf.id)?;
    let project = crate::cf::get_pages_project(&token, &cf.account_id, &site.deploy.project).await?.ok_or("Pages 项目还没建")?;
    let target = if project.subdomain.is_empty() { format!("{}.pages.dev", site.deploy.project) } else { project.subdomain.clone() };
    let zone = crate::cf::zone_for(&token, &site.domain).await?.ok_or_else(|| format!("在这个 Cloudflare 账号里没找到 {} 的 DNS", site.domain))?;
    let recs = crate::cf::dns_records(&token, &zone.id, &site.domain).await?;
    for r in &recs {
        crate::cf::delete_dns_record(&token, &zone.id, &r.id).await?;
    }
    crate::cf::ensure_dns_record(&token, &zone.id, "CNAME", &site.domain, &target, true).await?;
    Ok(format!("已把 {} 换成 CNAME → {target}（删掉了原来的 {} 条记录）。DNS 生效可能要几分钟", site.domain, recs.len()))
}

/// 「上线成功但域名打开还是旧站」的排查结果：域名实际返回的页面 vs 这次上线的 pages.dev，
/// 以及会抢在 Pages 前面回应这个域名的东西（没生效的 Zone、Worker 路由、Worker 自定义域名、别的 Pages 项目）。
#[derive(Serialize, Default)]
pub struct DomainCheck {
    pub domain: String,
    pub pages_url: String,
    pub live_title: String,
    pub pages_title: String,
    /// 两边都打开了、标题能比
    pub compared: bool,
    pub same: bool,
    /// 查过 Cloudflare 里占用这个域名的东西（quick 只比页面，或页面已经一致时不查）
    pub scanned: bool,
    pub issues: Vec<DomainIssue>,
    /// 这次自动处理掉的（「已删掉 Worker 路由 …」）
    pub fixed: Vec<String>,
    /// 没查成的项（多半是令牌少权限），不算问题
    pub notes: Vec<String>,
}

#[derive(Serialize, Default, Clone)]
pub struct DomainIssue {
    /// zone | route | worker_domain | pages | cache
    pub kind: String,
    pub text: String,
    /// 修复按钮的文字；空 = Pilot 修不了，要去 Cloudflare 后台
    pub fix: String,
    pub conn: String,
    pub target: String,
    /// 明确是在跟这个站点抢同一个域名、可以直接处理的（路由主机名正好是这个域名、Worker 自定义域名、别的 Pages 项目绑着它）
    pub auto: bool,
}

async fn page_title(url: &str) -> Result<String, String> {
    let resp = reqwest::Client::new()
        .get(url)
        .header("Cache-Control", "no-cache")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("打不开 {url}：{e}"))?;
    let html = resp.text().await.map_err(|e| e.to_string())?;
    let lower = html.to_ascii_lowercase();
    let t = lower
        .find("<title")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .and_then(|a| lower[a..].find("</title>").map(|b| html[a..a + b].trim().to_string()))
        .unwrap_or_default();
    Ok(t.replace("&amp;", "&").replace("&#39;", "'").replace("&quot;", "\""))
}

/// 公网上这个域名现在用的名称服务器（走 1.1.1.1 的 DoH，不看本机缓存）。
async fn live_ns(name: &str) -> Vec<String> {
    let Ok(resp) = reqwest::Client::new()
        .get(format!("https://cloudflare-dns.com/dns-query?name={name}&type=NS"))
        .header("Accept", "application/dns-json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    else {
        return Vec::new();
    };
    let v: serde_json::Value = resp.json().await.unwrap_or_default();
    let mut out: Vec<String> = v["Answer"].as_array().into_iter().flatten().filter(|a| a["type"].as_u64() == Some(2)).filter_map(|a| a["data"].as_str().map(|d| d.trim_end_matches('.').to_ascii_lowercase())).collect();
    out.sort();
    out
}

/// quick：只比页面；auto：查出明确在抢这个域名的东西就直接处理掉（Worker / 旧 Pages 项目本身都不删），再比一次。
#[tauri::command]
pub async fn site_domain_check(state: tauri::State<'_, AppState>, id: String, quick: Option<bool>, auto: Option<bool>) -> Result<DomainCheck, String> {
    let site = state.sites.get(&id)?;
    if site.domain.is_empty() {
        return Err("站点没有填写域名".into());
    }
    if auto.unwrap_or(false) {
        return Ok(domain_settle(&state.conns, &site).await);
    }
    Ok(domain_check(&state.conns, &site, quick.unwrap_or(false)).await)
}

/// 查一遍，有能直接处理的就处理掉，处理过再查一次。上线后（Pages）也会跑这个。
pub(crate) async fn domain_settle(conns: &crate::conns::ConnStore, site: &Site) -> DomainCheck {
    let first = domain_check(conns, site, false).await;
    if !first.compared || first.same {
        return first;
    }
    let mut fixed = Vec::new();
    let mut failed = Vec::new();
    for is in first.issues.iter().filter(|i| i.auto) {
        match domain_fix(conns, site, &is.kind, &is.conn, &is.target).await {
            Ok(m) => fixed.push(m),
            Err(e) => failed.push(format!("{}：{e}", is.fix)),
        }
    }
    if fixed.is_empty() {
        let mut out = first;
        out.notes.extend(failed);
        return out;
    }
    // 删路由、解绑之后边缘要几秒才切过去
    let mut out = first;
    for _ in 0..4 {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        out = domain_check(conns, site, false).await;
        if out.compared && out.same {
            break;
        }
    }
    out.fixed = fixed;
    out.notes.extend(failed);
    out
}

pub(crate) async fn domain_check(conns: &crate::conns::ConnStore, site: &Site, quick: bool) -> DomainCheck {
    let domain = site.domain.to_ascii_lowercase();
    let mut out = DomainCheck { domain: domain.clone(), ..Default::default() };
    // 站点用的那条 Cloudflare 连接排第一，其余连接也查（旧站可能在另一个账号里）
    let store = conns;
    let mut conns: Vec<_> = store.list().into_iter().filter(|c| c.kind == "cloudflare").collect();
    conns.sort_by_key(|c| c.id != site.deploy.cf_conn_id);
    if let Some(own) = conns.first().filter(|c| c.id == site.deploy.cf_conn_id) {
        if let Ok(token) = crate::keychain::get_key(&own.id) {
            if let Ok(Some(p)) = crate::cf::get_pages_project(&token, &own.account_id, &site.deploy.project).await {
                out.pages_url = format!("https://{}", if p.subdomain.is_empty() { format!("{}.pages.dev", site.deploy.project) } else { p.subdomain });
            }
        }
    }
    let live_url = format!("https://{domain}/");
    let live = page_title(&live_url);
    let pages = async {
        if out.pages_url.is_empty() {
            Err(String::new())
        } else {
            page_title(&format!("{}/", out.pages_url)).await
        }
    };
    let (live, pages) = tokio::join!(live, pages);
    match (&live, &pages) {
        (Ok(a), Ok(b)) => {
            out.compared = true;
            out.same = a == b;
        }
        (Err(e), _) => out.notes.push(e.clone()),
        (_, Err(e)) if !e.is_empty() => out.notes.push(e.clone()),
        _ => {}
    }
    out.live_title = live.unwrap_or_default();
    out.pages_title = pages.unwrap_or_default();
    // 只比页面（概览上自动检查），或者已经是这次上线的版本：不用再翻 Cloudflare
    if quick || (out.compared && out.same) {
        return out;
    }
    out.scanned = true;

    let mut seen_zones = std::collections::HashSet::new();
    let mut seen_accounts = std::collections::HashSet::new();
    let mut active_zone: Option<(String, String)> = None; // (conn, zone_id)
    for c in &conns {
        let Ok(token) = crate::keychain::get_key(&c.id) else { continue };
        let label = if c.remark.is_empty() { c.name.clone() } else { c.remark.clone() };
        if let Ok(Some(zone)) = crate::cf::zone_for(&token, &domain).await {
            if seen_zones.insert(zone.id.clone()) {
                let ns_live = live_ns(&zone.name).await;
                match crate::cf::zone_info(&token, &zone.id).await {
                    Ok((status, mut ns)) => {
                        ns.sort();
                        let ns_ok = ns_live.is_empty() || ns.is_empty() || ns == ns_live;
                        if status == "active" && ns_ok {
                            active_zone.get_or_insert((c.id.clone(), zone.id.clone()));
                        } else {
                            out.issues.push(DomainIssue {
                                kind: "zone".into(),
                                text: format!(
                                    "「{label}」这个 Cloudflare 账号里的 {} 没在生效（状态 {status}{}），在这里改的 DNS 和域名绑定都不起作用。域名实际用的名称服务器是 {}，在用的是另一个账号里的同名域名{}",
                                    zone.name,
                                    if ns.is_empty() { String::new() } else { format!("，分到的名称服务器 {}", ns.join(" / ")) },
                                    if ns_live.is_empty() { "（没查到）".into() } else { ns_live.join(" / ") },
                                    if conns.len() > 1 { "" } else { "：在 Pilot 里连上那个账号再检查，或去那个账号的后台改" }
                                ),
                                ..Default::default()
                            });
                        }
                    }
                    Err(e) => out.notes.push(format!("没读到 {} 的状态：{e}", zone.name)),
                }
                match crate::cf::worker_routes(&token, &zone.id).await {
                    Ok(routes) => {
                        for r in routes.into_iter().filter(|r| crate::cf::route_matches(&r.pattern, &domain)) {
                            out.issues.push(DomainIssue {
                                kind: "route".into(),
                                text: if r.script.is_empty() {
                                    format!("Worker 路由 {}（没有挂 Worker）：命中的请求不会交给 Pages", r.pattern)
                                } else {
                                    format!("Worker 路由 {} → Worker「{}」：这个地址的请求先交给这个 Worker，到不了 Pages", r.pattern, r.script)
                                },
                                fix: "删掉这条路由".into(),
                                conn: c.id.clone(),
                                target: format!("{}/{}", zone.id, r.id),
                                // `*x.com/*` 这类还会拦子域名，删了可能影响别的服务：留给你点
                                auto: !r.pattern.trim_start().starts_with('*'),
                            });
                        }
                    }
                    Err(e) => out.notes.push(format!("没读到 {} 的 Worker 路由（令牌可能缺「区域 → Workers 路由 → 读取」）：{e}", zone.name)),
                }
            }
        }
        if c.account_id.is_empty() || !seen_accounts.insert(c.account_id.clone()) {
            continue;
        }
        match crate::cf::worker_domains(&token, &c.account_id, &domain).await {
            Ok(ds) => {
                for d in ds {
                    out.issues.push(DomainIssue {
                        kind: "worker_domain".into(),
                        text: format!("{domain} 绑在「{label}」的 Worker「{}」上（Workers 自定义域名）：它比 DNS 记录优先，域名一直由这个 Worker 回应", d.service),
                        fix: format!("从 Worker「{}」解绑", d.service),
                        conn: c.id.clone(),
                        target: d.id,
                        auto: true,
                    });
                }
            }
            Err(e) => out.notes.push(format!("没读到「{label}」的 Workers 自定义域名（令牌可能缺「账户 → Workers 脚本 → 读取」）：{e}")),
        }
        match crate::cf::list_pages_projects(&token, &c.account_id).await {
            Ok(ps) => {
                for p in ps.into_iter().filter(|p| !(c.id == site.deploy.cf_conn_id && p.name == site.deploy.project) && p.domains.iter().any(|d| d.eq_ignore_ascii_case(&domain))) {
                    out.issues.push(DomainIssue {
                        kind: "pages".into(),
                        text: format!("{domain} 也绑在「{label}」的 Pages 项目「{}」上", p.name),
                        fix: format!("从「{}」去掉这个域名", p.name),
                        conn: c.id.clone(),
                        target: p.name,
                        auto: true,
                    });
                }
            }
            Err(e) => out.notes.push(format!("没读到「{label}」的 Pages 项目：{e}")),
        }
    }
    // 没找到别的占用、页面却还是旧的：多半是边缘缓存
    if out.compared && !out.same && out.issues.is_empty() {
        if let Some((conn, zone)) = active_zone {
            out.issues.push(DomainIssue {
                kind: "cache".into(),
                text: format!("没找到别的项目占用 {domain}，可能是 Cloudflare 边缘缓存着旧页面"),
                fix: "清除这个域名的缓存".into(),
                conn,
                target: zone,
                auto: true,
            });
        }
    }
    out
}

/// 执行排查结果里的一项修复（界面上确认过才调用）。改完顺手清一下这个域名的缓存。
#[tauri::command]
pub async fn site_domain_fix(state: tauri::State<'_, AppState>, id: String, kind: String, conn: String, target: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    domain_fix(&state.conns, &site, &kind, &conn, &target).await
}

pub(crate) async fn domain_fix(conns: &crate::conns::ConnStore, site: &Site, kind: &str, conn: &str, target: &str) -> Result<String, String> {
    let domain = site.domain.to_ascii_lowercase();
    let c = conns.get(conn)?;
    if c.kind != "cloudflare" {
        return Err("不是 Cloudflare 连接".into());
    }
    let token = crate::keychain::get_key(&c.id)?;
    let msg = match kind {
        "route" => {
            let (zone, route) = target.split_once('/').ok_or("路由参数不对")?;
            crate::cf::delete_worker_route(&token, zone, route).await?;
            "已删掉抢着回应这个域名的 Worker 路由（Worker 本身还在）".to_string()
        }
        "worker_domain" => {
            crate::cf::delete_worker_domain(&token, &c.account_id, target).await?;
            "已从 Worker 上解绑这个域名（Worker 本身还在）".to_string()
        }
        "pages" => {
            crate::cf::delete_pages_domain(&token, &c.account_id, target, &domain).await?;
            format!("已从 Pages 项目「{target}」去掉 {domain}")
        }
        "cache" => {
            crate::cf::purge_hosts(&token, target, &[domain.clone()]).await.map_err(|e| format!("清除缓存失败（令牌可能缺「区域 → 缓存清除」权限）：{e}"))?;
            return Ok(format!("已清除 {domain} 的缓存，刷新一下页面看看"));
        }
        _ => return Err("不认识的修复项".into()),
    };
    // 旧项目放手后，确保域名在这个站点的 Pages 项目上、DNS 指向它
    let mut extra = Vec::new();
    if matches!(site.deploy.mode.as_str(), "direct" | "pages_git" | "") {
        if let Ok(own) = conns.get(&site.deploy.cf_conn_id) {
            if let Ok(t) = crate::keychain::get_key(&own.id) {
                if let Ok(Some(p)) = crate::cf::get_pages_project(&t, &own.account_id, &site.deploy.project).await {
                    let _ = crate::cf::add_pages_domain(&t, &own.account_id, &site.deploy.project, &domain).await;
                    let target = if p.subdomain.is_empty() { format!("{}.pages.dev", site.deploy.project) } else { p.subdomain };
                    if let Ok(Some(zone)) = crate::cf::zone_for(&t, &domain).await {
                        if let Ok(recs) = crate::cf::dns_records(&t, &zone.id, &domain).await {
                            if recs.is_empty() && crate::cf::ensure_dns_record(&t, &zone.id, "CNAME", &domain, &target, true).await.is_ok() {
                                extra.push(format!("补上了 CNAME → {target}"));
                            }
                        }
                        if crate::cf::purge_hosts(&t, &zone.id, &[domain.clone()]).await.is_ok() {
                            extra.push("清了缓存".into());
                        }
                    }
                }
            }
        }
    }
    Ok(if extra.is_empty() { msg } else { format!("{msg}，{}", extra.join("、")) })
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
