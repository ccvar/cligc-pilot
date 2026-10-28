mod acp;
mod store_file;
mod typesafe;
mod jev_checks;
mod link_check;
mod monitor;
mod index_check;
mod background;
mod backup;
mod push;
mod revenue;
mod speed;
mod serp;
mod page_audit;
mod sitemap_fix;
mod agent;
mod bing;
mod api_agent;
mod api_execution;
mod api_billing;
mod api_profiles;
mod api_reasoning;
mod editorial;
mod fleet_cmds;
mod github;
mod brains;
mod conns;
mod bridge;
mod cf;
mod cf_templates;
mod convo;
mod data_cmds;
mod keychain;
mod launch_cmds;
mod managed;
mod managed_cmds;
mod node_boot;
mod ops_cmds;
mod path_env;
mod permit;
mod mail;
mod suggest;
mod oneshot;
mod blogger;
mod wpcom;
mod syndicate;
mod phone;
mod ssh;
mod sftp_transfer;
mod sftp_drop;
#[cfg(target_os = "macos")]
mod sftp_drag_macos;
mod static_server;
mod google;
mod google_oauth;
mod site_cmds;
mod site_config;
mod site_data;
mod site_deploy;
mod site_git;
mod site_icon;
mod site_stage;
mod window_memory;
mod site_github;
mod site_jobs;
mod site_make;
mod site_migrate;
mod migrate_extract;
mod migrate_cmds;
mod site_dynamic;
mod site_vps_host;
mod site_ops;
mod site_runner;
mod site_verify;
mod site_vps;
mod sites;
mod tools;
mod turn_checkpoint;
mod turn_deadline;
mod usage;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use convo::{Conversation, Message};

/// 应用句柄（setup 时放进来）：给拿不到 tauri State 的地方用，比如 API 执行器的工具里替用户启动「构建并上线」。
pub(crate) static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

struct AppState {
    conns: conns::ConnStore,
    convos: convo::ConvStore,
    runs: agent::RunRegistry,
    /// 应用数据目录（权限钩子资产 + 待批请求落在 <data_dir>/permit 下）。
    data_dir: PathBuf,
    /// 当前运行的本地预览（wrangler dev）进程；一次只跑一个。
    preview: Arc<Mutex<Option<PreviewHandle>>>,
    /// 活跃的 SSH PTY 会话（按 conn_id）。
    ssh: ssh::SshSessions,
    /// 站点（连接之上的运营对象）。
    sites: sites::SiteStore,
    /// 站点后台作业（构建上线 / 运营任务），每站同时只跑一个。
    site_jobs: site_jobs::Jobs,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 项目 slug：ASCII 小写字母/数字/连字符（CF 建站的 site_slug 复用为项目名）。
fn cf_project_slug(s: &str) -> String {
    let slug: String = s
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "site".into()
    } else {
        slug
    }
}

/// 附件文件名消毒：只留安全字符，去路径，限长。
fn sanitize_filename(name: &str) -> String {
    let base = std::path::Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned: String = cleaned.trim_matches('.').chars().take(80).collect();
    if cleaned.is_empty() {
        "file".into()
    } else {
        cleaned
    }
}

/// 计算本轮 cwd 但不建目录：CF 连接＝工作区下的项目目录；其它＝空串（调用方回退连接目录）。
/// 读路径（如附件缩略图）用它，避免只读操作把用户删掉的项目目录悄悄复活成幽灵项目。
fn work_dir_path(conn: &conns::Connection, site_slug: &str) -> String {
    if conn.kind == "github" {
        return github::repo_dir(&conn.skill_dir, site_slug)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    if conn.kind == "cloudflare" {
        std::path::Path::new(&conn.skill_dir)
            .join("projects")
            .join(cf_project_slug(site_slug))
            .to_string_lossy()
            .into_owned()
    } else {
        String::new()
    }
}

/// 项目目录里有没有东西（忽略隐藏文件）。CF 会话据此告诉模型「已有起点，先读再改」——
/// 引用了起始模板、或上一轮已经建过，都算。空目录才是真·从白纸开始。
fn dir_has_content(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| !e.file_name().to_string_lossy().starts_with('.'))
        })
        .unwrap_or(false)
}

/// codex 吃不到 `--plugin-dir`（只有会写 ~/.codex/config.toml 的全局 marketplace，与「一轮一进程」相抵触），
/// 给它在提示词里补一行设计规范的绝对路径兜底。claude/grok 走 plugin 注册，不需要这段。
fn design_fallback(sys: String, is_cf: bool, brain: &str, data_dir: &std::path::Path) -> String {
    if !is_cf || brain != "codex" {
        return sys;
    }
    let p = tools::design_skill_path(data_dir);
    format!("{sys}\n\n{}", agent::design_prompt(&p.to_string_lossy()))
}

/// 本轮 cwd：CF 连接＝该连接工作区下的项目目录（按需创建）；其它返回空由 run_turn 兜底。
fn resolve_work_dir(conn: &conns::Connection, site_slug: &str) -> Result<String, String> {
    let dir = work_dir_path(conn, site_slug);
    if !dir.is_empty() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("建项目目录失败: {e}"))?;
    }
    Ok(dir)
}

/// 自由对话运行时使用的合成连接 id。新会话的 Conversation.conn_id 记录侧栏创建位置，
/// 不再写这个值；仅为运行隔离、附件解析和旧数据迁移保留。
const WORKSPACE_CONNECTION_ID: &str = "__workspace__";

/// 自由对话不借用当前 Cloudflare/SSH 连接：这个合成连接只负责给智能体一个 cwd，
/// 不存在于 connections.json，也没有任何钥匙串凭据。
pub(crate) fn workspace_connection(work_dir: &str) -> conns::Connection {
    conns::Connection {
        id: WORKSPACE_CONNECTION_ID.into(),
        name: "自由对话".into(),
        remark: String::new(),
        kind: "workspace".into(),
        skill_dir: work_dir.into(),
        key_prefix: String::new(),
        key_kind: String::new(),
        account_id: String::new(),
        login: String::new(),
        ssh_host: String::new(),
        ssh_port: 0,
        ssh_user: String::new(),
        ssh_auth: String::new(),
        ssh_key_path: String::new(),
        ssh_fingerprint: String::new(),
        ssh_os: String::new(),
        ssh_os_id: String::new(),
        created_at: String::new(),
    }
}

/// 自由对话的 cwd：用户选了文件夹就只使用该目录；未选择时创建每会话独立的空白目录。
/// 外部目录必须真实存在且为目录，绝不代建一个看似成功的错误路径。
fn resolve_workspace_dir(
    data_dir: &std::path::Path,
    conv_id: &str,
    selected: &str,
) -> Result<String, String> {
    let selected = selected.trim();
    let dir = if selected.is_empty() {
        let dir = data_dir.join("workspaces").join(conv_id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建自由对话空间失败: {e}"))?;
        dir
    } else {
        let dir = std::fs::canonicalize(selected)
            .map_err(|_| "所选文件夹不可用，请重新选择后新建自由对话".to_string())?;
        if !dir.is_dir() {
            return Err("所选路径不是文件夹".into());
        }
        dir
    };
    Ok(dir.to_string_lossy().into_owned())
}

fn conversation_runtime(
    conns: &conns::ConnStore,
    sites: &sites::SiteStore,
    data_dir: &std::path::Path,
    conv: &Conversation,
) -> Result<(conns::Connection, String), String> {
    if conv.task_type == "site" {
        let (conn, work_dir, _, _) = site_cmds::site_runtime(sites, conns, &conv.site_slug)?;
        // 运营任务的会话跑在 Pilot 为它准备的独立工作树里（站点数据目录下）。
        let work_dir = site_cmds::site_work_override(sites, &conv.site_slug, &conv.workspace_dir).unwrap_or(work_dir);
        return Ok((conn, work_dir));
    }
    if conv.task_type == "workspace" {
        let work_dir = resolve_workspace_dir(data_dir, &conv.id, &conv.workspace_dir)?;
        return Ok((workspace_connection(&work_dir), work_dir));
    }
    let conn = conns.get(&conv.conn_id)?;
    let work_dir = resolve_work_dir(&conn, &conv.site_slug)?;
    Ok((conn, work_dir))
}

/// 附件命令既服务普通站点，也服务运行环境与侧栏归属分离的自由对话。
/// workspace 前端用会话 id 作为 project，借此只取回该会话已经确认过的 cwd；
/// 其它连接继续沿用原来的 conn + project 解析规则。
fn attachment_runtime(
    state: &AppState,
    conn_id: &str,
    project: &str,
    pending_workspace_dir: Option<&str>,
) -> Result<(conns::Connection, String), String> {
    // 新版 workspace 的 conn_id 是侧栏固定位置，不是运行连接；project 才是会话 id。
    // 先按 project 识别会话，可同时兼容旧版仍传 __workspace__ 的调用方。
    if let Some(conv) = state.convos.get(project) {
        if conv.task_type == "workspace" {
            if conn_id != WORKSPACE_CONNECTION_ID && conn_id != conv.conn_id {
                return Err("本地对话归属不匹配".into());
            }
            let (conn, _) = conversation_runtime(&state.conns, &state.sites, &state.data_dir, &conv)?;
            return Ok((conn, String::new()));
        }
    }
    // 站点会话：GitHub 来源按仓库解析检出目录；本地来源的合成连接 skill_dir 就是文件夹。
    if let Some(site_id) = conn_id.strip_prefix("site:") {
        let (conn, _, project, _) = site_cmds::site_runtime(&state.sites, &state.conns, site_id)?;
        return Ok((conn, project));
    }
    if conn_id == WORKSPACE_CONNECTION_ID {
        // 启动页还没发出第一条：前端预先定好了会话 id，附件先放进这条对话将来的工作目录
        if let Some(selected) = pending_workspace_dir {
            if !is_pending_conv_id(project) {
                return Err("自由对话 id 不对".into());
            }
            let dir = resolve_workspace_dir(&state.data_dir, project, selected)?;
            return Ok((workspace_connection(&dir), String::new()));
        }
        return Err("自由对话不存在".into());
    }
    Ok((state.conns.get(conn_id)?, project.to_string()))
}

#[tauri::command]
fn list_connections(state: tauri::State<'_, AppState>) -> Vec<conns::Connection> {
    state.conns.list()
}

#[tauri::command]
async fn remove_connection(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let store = state.conns.clone();
    let convos = state.convos.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.remove(&id)?; // 先删连接本体（钥匙串 + 技能目录）
        convos.remove_by_conn_id(&id) // 再级联删该连接下的所有对话
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_connection_remark(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    remark: String,
) -> Result<conns::Connection, String> {
    let store = state.conns.clone();
    tauri::async_runtime::spawn_blocking(move || store.set_remark(&conn_id, &remark))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn detect_brains() -> Result<brains::BrainsInfo, String> {
    Ok(brains::detect().await)
}

#[tauri::command]
fn list_api_profiles(state: tauri::State<'_, AppState>) -> Result<Vec<api_profiles::ApiProfileView>, String> {
    api_profiles::list(&state.data_dir)
}

fn api_profile_in_use(state: &AppState, id: &str) -> bool {
    let brain = format!("api:{id}");
    state.convos.list().iter().any(|c| c.brain == brain && c.status == "running")
}

#[tauri::command]
fn save_api_profile(
    state: tauri::State<'_, AppState>, profile: api_profiles::ApiProfileInput,
    api_key: Option<String>, confirm_endpoint_change: Option<bool>,
) -> Result<api_profiles::ApiProfileView, String> {
    if profile.id.as_deref().is_some_and(|id| api_profile_in_use(&state, id)) {
        return Err("该 API 连接有正在执行的会话，请停止后再编辑".into());
    }
    api_profiles::save(&state.data_dir, profile, api_key, confirm_endpoint_change.unwrap_or(false))
}

#[tauri::command]
fn delete_api_profile(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    if api_profile_in_use(&state, &id) { return Err("该 API 连接有正在执行的会话，请停止后再移除".into()); }
    // Keep conversations and task references. A missing profile fails explicitly;
    // it must never silently redirect an existing task to another account.
    api_profiles::delete(&state.data_dir, &id)
}

#[tauri::command]
async fn list_api_models(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<String>, String> {
    api_profiles::list_models(&state.data_dir, &id).await
}

#[tauri::command]
async fn test_api_profile(state: tauri::State<'_, AppState>, id: String) -> Result<api_profiles::ApiTestResult, String> {
    api_profiles::test(&state.data_dir, &id).await
}

#[tauri::command]
fn get_api_billing_settings(state: tauri::State<'_, AppState>, id: String) -> Result<api_billing::BillingSettings, String> {
    api_billing::settings(&state.data_dir, &id)
}

#[tauri::command]
fn save_api_billing_key(state: tauri::State<'_, AppState>, id: String, api_key: String) -> Result<api_billing::BillingSettings, String> {
    api_billing::save_key(&state.data_dir, &id, api_key)
}

#[tauri::command]
fn delete_api_billing_key(state: tauri::State<'_, AppState>, id: String) -> Result<api_billing::BillingSettings, String> {
    api_billing::delete_key(&state.data_dir, &id)
}

#[tauri::command]
async fn query_api_billing(state: tauri::State<'_, AppState>, id: String) -> Result<api_billing::BillingSnapshot, String> {
    api_billing::query(&state.data_dir, &id).await
}

fn validate_executor(data_dir: &std::path::Path, brain: &str, model: &str) -> Result<(), String> {
    if matches!(brain, "claude" | "codex" | "grok") { return Ok(()); }
    let id = brain.strip_prefix("api:").ok_or("未知模型服务")?;
    let profiles = api_profiles::list(data_dir)?;
    let profile = profiles.iter().find(|p| p.profile.id == id).ok_or("API 连接已移除，请重新选择模型服务")?;
    if !profile.profile.enabled { return Err("API 连接已停用".into()); }
    if !profile.key_configured { return Err("请先配置该连接的 API Key".into()); }
    if model.trim().is_empty() || !profile.profile.models.iter().any(|m| m == model) {
        return Err("该模型不在 API 连接的模型列表中，请先添加模型 ID".into());
    }
    Ok(())
}

/// 验证 Cloudflare API Token 并返回可用账号 + 域名（UI 据此让用户选默认账号）。
#[tauri::command]
async fn verify_cf_token(token: String) -> Result<cf::CfVerify, String> {
    cf::verify_token(&token).await
}

/// 连接 Cloudflare：再验证一遍 token（防被吊销）→ 存钥匙串 + 建工作区 + 写 connections.json。
#[tauri::command]
async fn connect_cloudflare(
    state: tauri::State<'_, AppState>,
    name: String,
    token: String,
    account_id: String,
) -> Result<conns::Connection, String> {
    cf::verify_token(&token).await?;
    let store = state.conns.clone();
    tauri::async_runtime::spawn_blocking(move || store.add_cloudflare(&name, &token, &account_id))
        .await
        .map_err(|e| e.to_string())?
}

/// 更新明确的一条 Cloudflare 连接：新 Token 必须仍包含原账号，避免用户误把另一账号的
/// Token 覆盖进去；连接 id 保留。
#[tauri::command]
async fn update_cloudflare_token(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    token: String,
) -> Result<conns::Connection, String> {
    let current = state.conns.get(&conn_id)?;
    if current.kind != "cloudflare" {
        return Err("这不是 Cloudflare 连接".into());
    }
    let verified = cf::verify_token(&token).await?;
    if !verified
        .accounts
        .iter()
        .any(|account| account.id == current.account_id)
    {
        return Err(format!(
            "新 Token 无法访问原 Cloudflare 账号 {}，未更新连接",
            current.account_id
        ));
    }
    let store = state.conns.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.replace_token(&conn_id, "cloudflare", &token)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- GitHub ----

/// 验证 fine-grained Token：返回账号与它能访问的仓库（含是否可写）。Token 不落盘。
#[tauri::command]
async fn verify_github_token(token: String) -> Result<github::GhVerify, String> {
    github::verify_token(&token).await
}

#[tauri::command]
async fn connect_github(
    state: tauri::State<'_, AppState>,
    name: String,
    token: String,
) -> Result<conns::Connection, String> {
    let verified = github::verify_token(&token).await?;
    let store = state.conns.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.add_github(&name, &token, &verified.login, verified.user_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 换 Token：新 Token 必须属于同一个 GitHub 账号；连接 id 与已检出的仓库保留。
#[tauri::command]
async fn update_github_token(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    token: String,
) -> Result<conns::Connection, String> {
    let current = state.conns.get(&conn_id)?;
    if current.kind != "github" {
        return Err("这不是 GitHub 连接".into());
    }
    let verified = github::verify_token(&token).await?;
    if verified.user_id.to_string() != current.account_id {
        return Err(format!("新 Token 属于 {}，不是原账号 {}，未更新连接", verified.login, current.login));
    }
    let store = state.conns.clone();
    tauri::async_runtime::spawn_blocking(move || store.replace_token(&conn_id, "github", &token))
        .await
        .map_err(|e| e.to_string())?
}

/// 用已保存的 Token 拉取可访问的仓库（新建会话时选仓库用）。
#[tauri::command]
async fn list_github_repos(
    state: tauri::State<'_, AppState>,
    conn_id: String,
) -> Result<Vec<github::GhRepo>, String> {
    let conn = state.conns.get(&conn_id)?;
    if conn.kind != "github" {
        return Err("这不是 GitHub 连接".into());
    }
    let token = tauri::async_runtime::spawn_blocking(move || keychain::get_key(&conn_id))
        .await
        .map_err(|e| e.to_string())??;
    Ok(github::verify_token(&token).await?.repos)
}

/// 本地已检出的仓库（`owner/repo`）。
#[tauri::command]
fn list_github_checkouts(state: tauri::State<'_, AppState>, conn_id: String) -> Vec<String> {
    match state.conns.get(&conn_id) {
        Ok(conn) if conn.kind == "github" => github::checkouts(&conn.skill_dir),
        _ => vec![],
    }
}

// ---- SSH 远程连接 ----

/// 由「表单原始输入」组装认证材料（试连时用，此时还没有连接记录/钥匙串条目）。
fn ssh_auth_from(
    auth: &str,
    user: &str,
    password: &str,
    key_path: &str,
    key_pass: &str,
) -> ssh::SshAuth {
    if auth == "key" {
        ssh::SshAuth {
            user: user.into(),
            password: None,
            key_path: Some(key_path.into()),
            key_pass: (!key_pass.is_empty()).then(|| key_pass.into()),
        }
    } else {
        ssh::SshAuth {
            user: user.into(),
            password: Some(password.into()),
            key_path: None,
            key_pass: None,
        }
    }
}

/// 试连。expect 为空（首次添加 / 还没记住指纹）：只取主机指纹、**不认证**，指纹交 UI 让人核对；
/// expect 非空：指纹一致才认证，不一致在认证前断开并把新指纹交回去（见 ssh::probe）。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn verify_ssh(
    state: tauri::State<'_, AppState>,
    host: String,
    port: u16,
    user: String,
    auth: String,
    password: String,
    key_path: String,
    key_pass: String,
    expect_fingerprint: String,
    conn_id: Option<String>,
) -> Result<ssh::SshProbe, String> {
    let mut a = ssh_auth_from(&auth, &user, &password, &key_path, &key_pass);
    // 编辑已有连接时密码/口令允许留空 = 沿用钥匙串里存着的那条（UI 不回显密码，也不该逼用户重打一遍）。
    // 只有在**认证方式没变**时才这么补：换了方式，旧条目存的是另一种东西（密码 vs 私钥口令）。
    if password.is_empty() && key_pass.is_empty() {
        if let Some(old) = conn_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|id| state.conns.get(id).ok())
            .filter(|c| c.kind == "ssh" && c.ssh_auth == auth)
        {
            let stored = ssh::auth_for_async(&old).await.ok();
            match (auth.as_str(), stored) {
                ("password", Some(s)) => a.password = s.password,
                ("key", Some(s)) => a.key_pass = s.key_pass,
                _ => {}
            }
        }
    }
    let expect =
        (!expect_fingerprint.trim().is_empty()).then(|| expect_fingerprint.trim().to_string());
    ssh::probe(&host, if port == 0 { 22 } else { port }, &a, expect).await
}

/// 新建 SSH 连接（指纹必须是 UI 里用户确认过的那个）。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn connect_ssh(
    state: tauri::State<'_, AppState>,
    name: String,
    host: String,
    port: u16,
    user: String,
    auth: String,
    key_path: String,
    secret: String,
    fingerprint: String,
) -> Result<conns::Connection, String> {
    let store = state.conns.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.add_ssh(
            &name,
            &host,
            port,
            &user,
            &auth,
            &key_path,
            &secret,
            &fingerprint,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 修改已有远程连接（指纹必须是 UI 里刚试连确认过的那个；secret 留空＝不动钥匙串里的密码/口令）。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn update_ssh(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    name: String,
    host: String,
    port: u16,
    user: String,
    auth: String,
    key_path: String,
    secret: String,
    fingerprint: String,
) -> Result<conns::Connection, String> {
    // 改完地址/认证后，表里那条旧会话（连的还是旧机器、用的还是旧凭据）必须作废。
    state.ssh.close(&conn_id).await;
    let store = state.conns.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.update_ssh(
            &conn_id,
            &name,
            &host,
            port,
            &user,
            &auth,
            &key_path,
            &secret,
            &fingerprint,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 从 /etc/os-release 解析 (PRETTY_NAME, ID)。拿不到 PRETTY_NAME 就退回 NAME+VERSION_ID。
fn parse_os_release(s: &str) -> (String, String) {
    let val = |key: &str| -> String {
        s.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
            .unwrap_or_default()
    };
    let id = val("ID");
    let pretty = val("PRETTY_NAME");
    if !pretty.is_empty() {
        return (pretty, id);
    }
    let name = val("NAME");
    if !name.is_empty() {
        let v = val("VERSION_ID");
        return (
            if v.is_empty() {
                name
            } else {
                format!("{name} {v}")
            },
            id,
        );
    }
    // 不是 systemd 系（或压根没有 os-release）：调用方会把 uname 的输出丢进来兜底。
    (
        s.trim()
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string(),
        id,
    )
}

/// 探远端系统版本并记进连接（UI 用它显示「Ubuntu 24.04」+ 发行版图标）。
/// 只在还没探过时调（探到就存下来，之后不再连）。
#[tauri::command]
async fn ssh_os_probe(
    state: tauri::State<'_, AppState>,
    conn_id: String,
) -> Result<conns::Connection, String> {
    let conn = state.conns.get(&conn_id)?;
    if conn.kind != "ssh" {
        return Err("这不是远程连接".into());
    }
    let auth = ssh::auth_for_async(&conn).await?;
    let expect = (!conn.ssh_fingerprint.is_empty()).then(|| conn.ssh_fingerprint.clone());
    state
        .ssh
        .ensure(&conn_id, &conn.ssh_host, conn.ssh_port, &auth, expect)
        .await?;
    // 非 Linux（或精简镜像）没有 os-release → 退回 uname 也好过什么都不显示。
    let out = state
        .ssh
        .exec(
            &conn_id,
            "cat /etc/os-release 2>/dev/null || uname -sr",
            20,
            false,
        ) // 后台探测，别拿噪音污染终端
        .await?;
    let (pretty, os_id) = parse_os_release(&out.stdout);
    let store = state.conns.clone();
    tauri::async_runtime::spawn_blocking(move || store.set_ssh_os(&conn_id, &pretty, &os_id))
        .await
        .map_err(|e| e.to_string())?
}

/// 取一次远端负载（CPU 累计计数 + 内存 + 根分区）。顶栏那三个数用它。
/// 没有现成会话就直接失败 —— 前端静默忽略，不为了三个数去偷偷登服务器。
#[tauri::command]
async fn ssh_stats(
    state: tauri::State<'_, AppState>,
    conn_id: String,
) -> Result<ssh::SshStats, String> {
    state.ssh.stats(&conn_id).await
}

/// 打开交互式 PTY shell，输出流给前端 xterm。连接时带上已确认指纹，不匹配即拒。
#[tauri::command]
async fn ssh_open_shell(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    cols: u32,
    rows: u32,
    on_event: Channel<ssh::SshEvent>,
) -> Result<(), String> {
    let conn = state.conns.get(&conn_id)?;
    if conn.kind != "ssh" {
        return Err("这不是远程连接".into());
    }
    let auth = ssh::auth_for_async(&conn).await?;
    let expect = (!conn.ssh_fingerprint.is_empty()).then(|| conn.ssh_fingerprint.clone());
    state
        .ssh
        .open_shell(
            &conn_id,
            &conn.ssh_host,
            conn.ssh_port,
            &auth,
            expect,
            cols.max(1),
            rows.max(1),
            on_event,
        )
        .await
}

/// 键盘输入（base64：可能敲出控制字节/非 UTF-8 序列）。
#[tauri::command]
async fn ssh_input(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    b64: String,
) -> Result<(), String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| format!("输入解码失败: {e}"))?;
    state.ssh.input(&conn_id, &bytes).await
}

#[tauri::command]
async fn ssh_resize(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    state.ssh.resize(&conn_id, cols.max(1), rows.max(1)).await
}

#[tauri::command]
async fn ssh_close(state: tauri::State<'_, AppState>, conn_id: String) -> Result<(), String> {
    state.ssh.close(&conn_id).await;
    Ok(())
}

// ---- SSH 远程文件（SFTP，复用已打开的终端会话，不重新握手） ----

/// 在线编辑的大小闸门：整文件进内存 + base64 过 IPC，2MB 之上就该用下载了。
const SFTP_EDIT_MAX: u64 = 2 * 1024 * 1024;

/// SFTP 命令的会话保障：过去 SFTP 只 `g.get(conn_id)`，要**靠终端先把会话建起来** ——
/// 于是「文件面板抢在终端连上之前加载」就卡死在「会话未打开：请先连接远程终端」，
/// 而且前端那个 $effect 加载失败不重试，终端后来连上了文件面板也不会自己回来（真踩过）。
/// 这里让每条 SFTP 命令**自己 ensure**：会话已在就是一次 map 查询直接返回（ensure 内部对活着的
/// 会话早退，不重连、不读凭据）；没在就按连接的凭据+指纹连上（和 open_shell 同一套）。
/// 文件浏览是用户的直接动作，该连就连——不像 stats 那样为了三个数字克制着不登录。
async fn ensure_ssh(state: &AppState, conn_id: &str) -> Result<(), String> {
    if state.ssh.is_open(conn_id).await {
        return Ok(());
    }
    let conn = state.conns.get(conn_id)?;
    if conn.kind != "ssh" {
        return Err("这不是远程连接".into());
    }
    let auth = ssh::auth_for_async(&conn).await?;
    let expect = (!conn.ssh_fingerprint.is_empty()).then(|| conn.ssh_fingerprint.clone());
    state
        .ssh
        .ensure(conn_id, &conn.ssh_host, conn.ssh_port, &auth, expect)
        .await
}

#[tauri::command]
async fn sftp_list(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    path: String,
) -> Result<Vec<ssh::SftpEntry>, String> {
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.list_dir(&conn_id, &path).await
}

/// 解析起始目录（"." → 真实 home 的绝对路径）。
#[tauri::command]
async fn sftp_home(state: tauri::State<'_, AppState>, conn_id: String) -> Result<String, String> {
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.real_path(&conn_id, ".").await
}

/// 读整个文件给在线编辑器。返回 base64：文本是否合法 UTF-8 由前端判定（TextDecoder fatal）。
#[tauri::command]
async fn sftp_read(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    path: String,
) -> Result<String, String> {
    use base64::Engine as _;
    ensure_ssh(&state, &conn_id).await?;
    let bytes = state.ssh.read_file(&conn_id, &path, SFTP_EDIT_MAX).await?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[tauri::command]
async fn sftp_write(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    path: String,
    b64: String,
) -> Result<(), String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| format!("内容解码失败: {e}"))?;
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.write_file(&conn_id, &path, &bytes).await
}

#[tauri::command]
async fn sftp_rename(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    from: String,
    to: String,
) -> Result<(), String> {
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.rename(&conn_id, &from, &to).await
}

#[tauri::command]
async fn sftp_copy(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    from: String,
    to: String,
) -> Result<(), String> {
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.copy_path(&conn_id, &from, &to).await
}

#[tauri::command]
async fn sftp_remove(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    path: String,
    dir: bool,
) -> Result<(), String> {
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.remove(&conn_id, &path, dir).await
}

#[tauri::command]
async fn sftp_mkdir(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    path: String,
) -> Result<(), String> {
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.mkdir(&conn_id, &path).await
}

#[tauri::command]
async fn sftp_create_file(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    path: String,
) -> Result<(), String> {
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.create_file(&conn_id, &path).await
}

#[tauri::command]
async fn sftp_download(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    remote: String,
    local: String,
) -> Result<u64, String> {
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.download(&conn_id, &remote, &local).await
}

#[tauri::command]
async fn sftp_upload(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    local: String,
    remote: String,
) -> Result<u64, String> {
    ensure_ssh(&state, &conn_id).await?;
    state.ssh.upload(&conn_id, &local, &remote).await
}

/// 把粘贴/拖拽的文件存进当前会话的工作目录 uploads/ 下，返回相对路径（AI 可直接读取）。
/// CF 会话＝项目目录；自由对话＝会话工作目录。
/// 前端 crypto.randomUUID() 生成的会话 id（拿来拼目录，只认这个格式）
fn is_pending_conv_id(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// 拖入 / 粘贴的附件上限：内容要整个经过前端（读进内存再传过来），太大会把界面卡住。
/// 点「+」选的文件走 `save_attachment_path`，后端按路径直接复制，不设上限。
const ATTACH_RAW_MAX: usize = 200 * 1024 * 1024;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachMeta {
    conn_id: String,
    project: String,
    /// 启动页给还没建的自由对话加附件时传（空串＝没选文件夹）；其它情况为空
    #[serde(default)]
    workspace_dir: Option<String>,
    filename: String,
}

/// 拖入 / 粘贴的文件存进当前会话工作目录的 uploads/，返回相对路径（AI 可直接读取）。
/// 内容走原始字节（invoke 的 body 直接是 Uint8Array），不再转成 JSON 数字数组——那样 1 个字节要变成好几个字符，
/// 25MB 就要传 100MB 左右的文本。会话信息放在 `x-pilot-attach` 头里（JSON 再 base64，文件名可能有中文）。
#[tauri::command]
async fn save_attachment(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("附件内容格式不对".into());
    };
    if data.len() > ATTACH_RAW_MAX {
        return Err("文件太大（拖入 / 粘贴上限 200MB）：更大的文件请点输入框左下角的「+」选择".into());
    }
    let meta: AttachMeta = {
        use base64::Engine as _;
        let raw = request
            .headers()
            .get("x-pilot-attach")
            .and_then(|v| v.to_str().ok())
            .ok_or("缺少附件信息")?;
        let json = base64::engine::general_purpose::STANDARD.decode(raw).map_err(|_| "附件信息格式不对")?;
        serde_json::from_slice(&json).map_err(|e| format!("附件信息格式不对: {e}"))?
    };
    let (conn, runtime_project) = attachment_runtime(&state, &meta.conn_id, &meta.project, meta.workspace_dir.as_deref())?;
    let data = data.clone();
    // 大文件写盘别占着主线程 / tokio 工作线程。
    tauri::async_runtime::spawn_blocking(move || {
        let target = attachment_dest(&conn, &runtime_project, &meta.filename)?;
        std::fs::write(&target, &data).map_err(|e| format!("保存附件失败: {e}"))?;
        Ok(upload_rel(&target))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
struct SavedAttachment {
    /// 工作目录里的相对路径（uploads/xxx）
    path: String,
    /// 图片的缩略图（data URI）；不是图片或太大就是空串
    preview: String,
}

/// 点「+」选的文件：后端按路径直接复制进 uploads/，不经过前端，所以没有大小限制。
#[tauri::command]
async fn save_attachment_path(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    project: String,
    workspace_dir: Option<String>,
    src: String,
) -> Result<SavedAttachment, String> {
    let (conn, runtime_project) = attachment_runtime(&state, &conn_id, &project, workspace_dir.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || {
        let src = std::path::PathBuf::from(&src);
        let meta = std::fs::metadata(&src).map_err(|e| format!("读不到这个文件: {e}"))?;
        if meta.is_dir() {
            return Err("选的是文件夹：请选文件".into());
        }
        let filename = src.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        let target = attachment_dest(&conn, &runtime_project, &filename)?;
        std::fs::copy(&src, &target).map_err(|e| format!("复制附件失败: {e}"))?;
        let preview = match image_mime(&filename) {
            Some(mime) if meta.len() <= 8_000_000 => std::fs::read(&target)
                .map(|d| {
                    use base64::Engine as _;
                    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(d))
                })
                .unwrap_or_default(),
            _ => String::new(),
        };
        Ok(SavedAttachment { path: upload_rel(&target), preview })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn image_mime(name: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => return None,
    })
}

#[cfg(test)]
mod attachment_dest_tests {
    use super::*;

    #[test]
    fn same_name_gets_numbered_and_path_is_relative() {
        let dir = tempfile::tempdir().unwrap();
        let conn = workspace_connection(&dir.path().to_string_lossy());
        let a = attachment_dest(&conn, "", "shot.png").unwrap();
        std::fs::write(&a, b"x").unwrap();
        let b = attachment_dest(&conn, "", "shot.png").unwrap();
        assert_eq!(upload_rel(&a), "uploads/shot.png");
        assert_eq!(upload_rel(&b), "uploads/shot-1.png");
        assert_eq!(image_mime("a.JPG"), Some("image/jpeg"));
        assert_eq!(image_mime("a.zip"), None);
    }
}

/// uploads/ 下的相对路径（前端和 AI 都用这个）
fn upload_rel(target: &std::path::Path) -> String {
    format!("uploads/{}", target.file_name().and_then(|n| n.to_str()).unwrap_or("file"))
}

/// 附件要放到哪：工作目录下的 uploads/，重名就加 -1、-2…
fn attachment_dest(
    conn: &conns::Connection,
    runtime_project: &str,
    filename: &str,
) -> Result<std::path::PathBuf, String> {
    let dir = {
        let w = resolve_work_dir(conn, runtime_project)?;
        if w.is_empty() {
            conn.skill_dir.clone()
        } else {
            w
        }
    };
    if dir.is_empty() {
        return Err("没有可用的工作目录".into());
    }
    let up = std::path::Path::new(&dir).join("uploads");
    std::fs::create_dir_all(&up).map_err(|e| format!("建 uploads 目录失败: {e}"))?;
    let safe = sanitize_filename(filename);
    let mut target = up.join(&safe);
    if target.exists() {
        let p = std::path::Path::new(&safe);
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        for i in 1..1000 {
            let cand = up.join(format!("{stem}-{i}{ext}"));
            if !cand.exists() {
                target = cand;
                break;
            }
        }
    }
    Ok(target)
}

/// 校验并解析消息里提到的文件路径为绝对路径。相对路径按工作目录解析；绝对路径
/// （AI 常直接给全路径）也接，但 canonicalize 后必须仍在工作目录内——边界与相对
/// 路径完全一致，工作目录之外一律拒绝。只读解析不建目录（目录被删就自然报错，调用方降级）。
fn resolve_in_workdir(
    conn: &conns::Connection,
    project: &str,
    raw: &str,
) -> Result<std::path::PathBuf, String> {
    let dir = {
        let w = work_dir_path(conn, project);
        if w.is_empty() {
            conn.skill_dir.clone()
        } else {
            w
        }
    };
    if dir.is_empty() {
        return Err("没有可用的工作目录".into());
    }
    let root = std::fs::canonicalize(&dir).map_err(|e| format!("工作目录不可用: {e}"))?;
    let raw = raw.trim();
    let is_abs = raw.starts_with('/')
        || raw.starts_with('\\')
        || (raw.len() > 2 && raw.as_bytes()[1] == b':' && raw.as_bytes()[0].is_ascii_alphabetic());
    let p = if is_abs {
        if raw.len() > 1024 {
            return Err("路径不合法".into());
        }
        // 本机不存在的「绝对路径」再按工作目录相对解析一次：助手常写站点口径的
        // /uploads/xx.svg，它真身就在 <工作目录>/uploads/ 下。
        let exact = match std::fs::canonicalize(raw) {
            Ok(p) => Ok(p),
            Err(e) => std::fs::canonicalize(root.join(raw.trim_start_matches(['/', '\\'])))
                .map_err(|_| format!("读取失败: {e}")),
        };
        match exact {
            Ok(p) if p.starts_with(&root) => p,
            // 智能体自己的产物目录也放行（codex 生图固定写 ~/.codex/generated_images）
            Ok(p) if agent_output_roots().iter().any(|r| p.starts_with(r)) => p,
            Ok(_) => return Err("文件在工作目录之外".into()),
            // 精确解析不到（模型常把真实写在工作目录里的文件链接成想象中的前缀，
            // 如 /mnt/data/xx.webp、sandbox:…）：按文件名在工作目录内搜，唯一命中才用。
            Err(e) => find_by_basename(&root, raw).ok_or(e)?,
        }
    } else {
        let rel = raw.trim_start_matches("./");
        if rel.is_empty() || rel.len() > 512 || rel.contains(':') || rel.contains("..") {
            return Err("路径不合法".into());
        }
        match std::fs::canonicalize(root.join(rel)) {
            Ok(p) if p.starts_with(&root) => p,
            Ok(_) => return Err("路径不合法".into()),
            Err(e) => find_by_basename(&root, rel).ok_or_else(|| format!("读取失败: {e}"))?,
        }
    };
    Ok(p)
}

/// 智能体自己的产物目录（工作目录之外的只读预览白名单）：codex 生图工具固定写到
/// ~/.codex/generated_images（CODEX_HOME 可改基目录）。只放行这一处，不放开家目录其他内容。
fn agent_output_roots() -> Vec<std::path::PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|h| std::path::Path::new(&h).join(".codex"))
        })
        .map(|b| b.join("generated_images"))
        .and_then(|p| std::fs::canonicalize(p).ok())
        .into_iter()
        .collect()
}

/// 按文件名在工作目录内浅搜（深度≤4、扫描≤2000 项；跳过 .xx 隐藏目录和 node_modules）。
/// **恰好一个**同名文件才返回——两个以上宁可不猜；命中后仍 canonicalize + 界内校验
/// （防目录内的符号链接指到外面）。
fn find_by_basename(root: &std::path::Path, raw: &str) -> Option<std::path::PathBuf> {
    let name = raw.trim().rsplit(['/', '\\']).next()?.trim();
    if name.is_empty() || !name.contains('.') {
        return None;
    }
    let mut found: Option<std::path::PathBuf> = None;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            if seen > 2000 {
                return None; // 目录太大：放弃兜底，不做半截扫描的猜测
            }
            let p = e.path();
            let fname = e.file_name();
            let fname = fname.to_string_lossy();
            // 构建产物（dist / build 等）是源文件的副本：跳过，否则 public/logo.svg 和 dist/logo.svg 会被当成「同名多处」
            // 只跳过项目根目录下的构建目录（brand/out 这种内容文件夹照常搜）
            if fname.starts_with('.') || fname == "node_modules" || (depth == 0 && matches!(fname.as_ref(), "dist" | "build" | "out" | "target")) {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth < 4 {
                    stack.push((p, depth + 1));
                }
            } else if fname == name {
                if found.is_some() {
                    return None; // 同名多处：有歧义，不猜
                }
                found = Some(p);
            }
        }
    }
    let p = std::fs::canonicalize(found?).ok()?;
    p.starts_with(root).then_some(p)
}

#[cfg(test)]
mod os_release_tests {
    use super::parse_os_release;

    #[test]
    fn reads_pretty_name_and_id() {
        // Ubuntu（真机 24.04 的形状）
        let (p, id) = parse_os_release(
            "PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nNAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nID=ubuntu\nID_LIKE=debian\n",
        );
        assert_eq!(p, "Ubuntu 24.04.1 LTS");
        assert_eq!(id, "ubuntu");
        // Debian
        let (p, id) =
            parse_os_release("PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nID=debian\n");
        assert_eq!(p, "Debian GNU/Linux 12 (bookworm)");
        assert_eq!(id, "debian");
        // Alpine 没有 PRETTY_NAME → 退回 NAME + VERSION_ID
        let (p, id) = parse_os_release("NAME=\"Alpine Linux\"\nID=alpine\nVERSION_ID=3.20.3\n");
        assert_eq!(p, "Alpine Linux 3.20.3");
        assert_eq!(id, "alpine");
        // 单引号 + 无 VERSION_ID
        let (p, _) = parse_os_release("NAME='Arch Linux'\nID=arch\n");
        assert_eq!(p, "Arch Linux");
    }

    #[test]
    fn falls_back_to_uname_output() {
        // 没有 os-release 的机器：命令退回 `uname -sr`，第一行就是全部信息
        let (p, id) = parse_os_release("Linux 6.8.0-45-generic\n");
        assert_eq!(p, "Linux 6.8.0-45-generic");
        assert_eq!(id, "");
        // 什么都没拿到也别炸
        assert_eq!(parse_os_release(""), (String::new(), String::new()));
        assert_eq!(parse_os_release("   \n\n"), (String::new(), String::new()));
    }

    /// ID 是子串匹配的重灾区：别让 VERSION_ID / ID_LIKE 冒充 ID。
    #[test]
    fn does_not_confuse_similar_keys() {
        let (_, id) = parse_os_release("VERSION_ID=\"22.04\"\nID_LIKE=debian\nID=ubuntu\n");
        assert_eq!(id, "ubuntu");
        let (_, id) = parse_os_release("ID_LIKE=\"rhel fedora\"\nVERSION_ID=9\n");
        assert_eq!(id, "", "只有 ID_LIKE 时不该把它当成 ID");
    }
}

#[cfg(test)]
mod workdir_tests {
    use super::*;

    /// 预览端口被别人占着时的行为——这正是「预览打开的不是对应页面」那个 bug。
    ///
    /// ★ 占位用**通配地址** `0.0.0.0`（真实现场就是这样：别的 app 绑 `*:8788`）。
    /// 这一条是刻意的：第一版用 bind 探端口，在通配占位下会被 SO_REUSEADDR 骗过去说「空闲」，
    /// 于是挑中一个有人在听的端口。用 127.0.0.1 占位的测试**发现不了**这个 bug。
    #[test]
    fn preview_port_dodges_a_taken_port() {
        let squatter = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let taken = squatter.local_addr().unwrap().port();
        assert!(
            port_taken(taken),
            "通配占位下必须判定为「被占」（bind 探测在这里会说谎）"
        );

        // 用一个**真需要 wrangler** 的目录来验让端口（纯静态站走内建服务器，是另一条路，见下面那条测试）
        let d = std::env::temp_dir().join(format!("pv-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("functions")).unwrap();

        // wrangler 路径：必须自动让开，绝不能还用那个被占的端口
        let (cmd, port) = plan_cmd(&d, None, "", taken);
        assert_ne!(port, taken, "端口被占还硬用 → 预览窗里就是别人的页面");
        assert!(
            cmd.contains(&format!("--port {port}")),
            "命令里的端口要跟返回的一致: {cmd}"
        );
        assert!(cmd.contains("wrangler pages dev ."));

        // 自定义 dev 命令：端口写死在命令里，改不了 → 必须报错而不是开个窗给别人的页面
        let e = preview_plan(&d, Some("npm run dev".into()), "", taken)
            .err()
            .unwrap();
        assert!(e.contains("已被别的程序占用"), "得说清是端口被占: {e}");

        // 端口没被占时：原样使用，产物目录也带上
        drop(squatter);
        // 上面的 port_taken 会实际建立一次 TCP 连接。macOS 偶尔会在 listener
        // 关闭后的极短窗口里仍让下一次 connect 成功，等内核确认端口释放再断言，
        // 避免把 socket 关闭时序误报成端口选择回归。
        for _ in 0..20 {
            if !port_taken(taken) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!port_taken(taken), "测试占位监听关闭后端口应释放");
        let (cmd, port) = plan_cmd(&d, None, "dist", taken);
        assert_eq!(port, taken);
        assert!(cmd.contains("wrangler pages dev dist"));
        std::fs::remove_dir_all(&d).ok();
    }

    /// 取 Cmd 计划；不是 Cmd 就炸（测试里用，省得每处 match）。
    fn plan_cmd(d: &std::path::Path, dev: Option<String>, out: &str, port: u16) -> (String, u16) {
        match preview_plan(d, dev, out, port).unwrap() {
            PreviewPlan::Wrangler(c, p) | PreviewPlan::Custom(c, p) => (c, p),
            PreviewPlan::Static(..) => panic!("这个目录该走 wrangler，却被判成了静态"),
        }
    }

    /// ★ 静态站**不该逼人装 wrangler**（用户原话：「一定要安装 wrangler 吗」——Windows 上没 Node
    /// 就直接卡死在「25 秒没起来」）。反过来，真用到 Workers/Pages 特性的**不许降级**：
    /// 静态服务器不认 functions/_headers/_redirects，硬服务会给出「能打开但和线上不一样」的假象。
    #[test]
    fn static_sites_never_need_wrangler_but_workers_features_still_do() {
        let base = std::env::temp_dir().join(format!("pvs-{}", uuid::Uuid::new_v4()));

        // 内置模板长这样：单文件静态 HTML
        let plain = base.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("index.html"), "<h1>hi</h1>").unwrap();
        match preview_plan(&plain, None, "", 8788).unwrap() {
            PreviewPlan::Static(dir, _) => assert_eq!(dir, plain, "静态站该由内建服务器伺候"),
            PreviewPlan::Wrangler(c, _) | PreviewPlan::Custom(c, _) => panic!("纯静态站不该拉 wrangler: {c}"),
        }

        // 有 Workers/Pages 特性 → 必须仍然交给 wrangler
        for f in ["_worker.js", "_headers", "_redirects", "wrangler.toml"] {
            let d = base.join(format!("w-{}", f.replace('.', "-")));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("index.html"), "x").unwrap();
            std::fs::write(d.join(f), "x").unwrap();
            let (cmd, _) = plan_cmd(&d, None, "", 8788);
            assert!(
                cmd.contains("wrangler pages dev"),
                "{f} 必须走 wrangler: {cmd}"
            );
        }

        // pilot.json 自定义了 dev → 永远听用户的，别自作主张换成内建服务器。
        // 端口得现找一个空闲的：8788 在开发机上往往正被真预览占着，写死会让测试看机器脸色。
        let free = free_port(9700).expect("找不到空闲端口跑这条测试");
        let (cmd, _) = plan_cmd(&plain, Some("npm run dev".into()), "", free);
        assert_eq!(cmd, "npm run dev", "自定义 dev 命令优先级最高");
        // 但它必须被标成「自定义」——执行前要经人确认、不给 token；wrangler 那条才是我们自己的
        assert!(matches!(preview_plan(&plain, Some("npm run dev".into()), "", free), Ok(PreviewPlan::Custom(..))));
        let w = base.join("w-functions");
        std::fs::create_dir_all(w.join("functions")).unwrap();
        assert!(matches!(preview_plan(&w, None, "", free), Ok(PreviewPlan::Wrangler(..))));

        std::fs::remove_dir_all(&base).ok();
    }

    /// compat date：项目没声明就兜一个（不兜＝wrangler 默认用「今天」→ workerd 不支持 → **白屏**）；
    /// 项目自己声明了就别插手（CLI 参数会盖过 wrangler.toml，硬给等于把人家降级）。
    /// 注意：只有**走 wrangler 的**才有这个问题，所以这里的目录必须是需要 wrangler 的。
    #[test]
    fn preview_compat_date_only_when_project_declares_none() {
        let d = std::env::temp_dir().join(format!("pvc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("functions")).unwrap(); // 逼它走 wrangler

        // 没声明 → 必须兜底，否则预览白屏
        let (cmd, _) = plan_cmd(&d, None, "", 8788);
        assert!(
            cmd.contains(&format!("--compatibility-date={FALLBACK_COMPAT_DATE}")),
            "没声明 compat date 就得兜底，否则 wrangler 默认今天→workerd 起不来→白屏: {cmd}"
        );

        // 项目声明了 → 绝不能再传（传了会盖掉人家的）
        std::fs::write(
            d.join("wrangler.toml"),
            "compatibility_date = \"2026-01-01\"\n",
        )
        .unwrap();
        assert!(declares_compat_date(&d));
        let (cmd, _) = plan_cmd(&d, None, "", 8788);
        assert!(
            !cmd.contains("--compatibility-date"),
            "项目声明了就别插手: {cmd}"
        );

        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn url_port_parses() {
        assert_eq!(url_port("http://127.0.0.1:8789"), Some(8789));
        assert_eq!(url_port("http://127.0.0.1:8789/"), Some(8789));
        assert_eq!(url_port("nonsense"), None);
    }

    fn test_conn(dir: &str) -> conns::Connection {
        serde_json::from_value(serde_json::json!({
            "id": "t", "name": "t", "kind": "workspace", "skill_dir": dir,
            "key_prefix": "", "key_kind": "", "created_at": "",
        }))
        .expect("test connection")
    }

    /// 相对 / 绝对 / 站点口径绝对(/uploads/..) 路径都必须解析进工作目录；越界与逃逸一律拒绝。
    #[test]
    fn resolve_in_workdir_boundaries() {
        let tmp =
            std::env::temp_dir().join(format!("pilot-workdir-test-{}", std::process::id()));
        let up = tmp.join("uploads");
        std::fs::create_dir_all(&up).unwrap();
        std::fs::write(up.join("a.svg"), "<svg/>").unwrap();
        let conn = test_conn(tmp.to_str().unwrap());

        assert!(
            resolve_in_workdir(&conn, "", "uploads/a.svg").is_ok(),
            "相对路径"
        );
        let abs = tmp.join("uploads").join("a.svg");
        assert!(
            resolve_in_workdir(&conn, "", abs.to_str().unwrap()).is_ok(),
            "工作目录内绝对路径"
        );
        assert!(
            resolve_in_workdir(&conn, "", "/uploads/a.svg").is_ok(),
            "站点口径绝对路径回退相对解析"
        );
        assert!(
            resolve_in_workdir(&conn, "", "/etc/hosts").is_err(),
            "工作目录之外的绝对路径"
        );
        assert!(resolve_in_workdir(&conn, "", "../x").is_err(), "相对逃逸");
        assert!(
            resolve_in_workdir(&conn, "", "uploads/missing.svg").is_err(),
            "不存在的文件"
        );

        // 文件名兜底：模型把真实文件链接成想象中的前缀（/mnt/data/、sandbox: 剥壳后）
        // 构建产物里的同名副本不算歧义
        std::fs::create_dir_all(tmp.join("dist/brand/out")).unwrap();
        std::fs::write(tmp.join("dist/brand/out/preview.webp"), "x").unwrap();
        std::fs::create_dir_all(tmp.join("brand/out")).unwrap();
        std::fs::write(tmp.join("brand/out/preview.webp"), "x").unwrap();
        assert!(
            resolve_in_workdir(&conn, "", "/mnt/data/preview.webp").is_ok(),
            "想象前缀按文件名找回"
        );
        assert!(
            resolve_in_workdir(&conn, "", "outputs/preview.webp").is_ok(),
            "相对路径写错也找回"
        );
        // 多处同名 → 有歧义不猜
        std::fs::write(tmp.join("uploads/preview.webp"), "y").unwrap();
        assert!(
            resolve_in_workdir(&conn, "", "/mnt/data/preview.webp").is_err(),
            "同名多处不猜"
        );
        // 真不存在仍报错
        assert!(
            resolve_in_workdir(&conn, "", "/mnt/data/nothing.webp").is_err(),
            "真不存在"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// codex 生图产物目录（CODEX_HOME/generated_images）在工作目录之外也可预览；
    /// 家目录其他位置仍拒绝（resolve_in_workdir_boundaries 里的 /etc/hosts 用例）。
    #[test]
    fn codex_generated_images_root_allowed() {
        let ch =
            std::env::temp_dir().join(format!("pilot-codexhome-test-{}", std::process::id()));
        std::fs::create_dir_all(ch.join("generated_images").join("sub")).unwrap();
        let img = ch.join("generated_images").join("sub").join("gen.png");
        std::fs::write(&img, "p").unwrap();
        std::env::set_var("CODEX_HOME", &ch);

        let wd =
            std::env::temp_dir().join(format!("pilot-workdir2-test-{}", std::process::id()));
        std::fs::create_dir_all(&wd).unwrap();
        let conn = test_conn(wd.to_str().unwrap());
        assert!(
            resolve_in_workdir(&conn, "", img.to_str().unwrap()).is_ok(),
            "生图目录放行"
        );

        std::env::remove_var("CODEX_HOME");
        std::fs::remove_dir_all(&ch).ok();
        std::fs::remove_dir_all(&wd).ok();
    }

    /// 安装失败摘要：优先脚本 stdout 末行；stderr 异常首行剥掉 PowerShell 命令回显前缀。
    #[test]
    fn install_err_brief_extracts_script_words() {
        // 复刻用户实拍：脚本把真实原因写在 stdout，PS 异常首行是整条命令回显 + 通用消息
        let stdout = "Downloading Claude Code...\nFailed to download from https://storage.googleapis.com/...: timeout\n".as_bytes();
        let stderr = "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor 3072; irm https://claude.ai/install.ps1 | iex : Installation failed (exit code 1)\nAt line:1 char:1\n+ FullyQualifiedErrorId : xxx\n".as_bytes();
        let m = install_err_brief(stdout, stderr);
        assert!(m.contains("Failed to download"), "应含脚本自述原因: {m}");
        assert!(m.contains("Installation failed"), "应含异常消息: {m}");
        assert!(!m.contains("SecurityProtocol"), "不应带命令回显: {m}");

        // stdout 为空时退回 stderr；两者全空给未知错误
        assert!(install_err_brief(b"", b"curl: (7) Failed to connect\n").contains("curl: (7)"));
        assert_eq!(install_err_brief(b"", b""), "未知错误");
    }

    /// WinINET ProxyServer 解析：单一 host:port / 分协议优先 https / 仅 ftp、socks 不猜 / 空与畸形。
    #[test]
    fn win_proxy_server_parse() {
        assert_eq!(
            parse_win_proxy_server("127.0.0.1:7890").as_deref(),
            Some("127.0.0.1:7890")
        );
        assert_eq!(
            parse_win_proxy_server("  10.0.0.1:8080  ").as_deref(),
            Some("10.0.0.1:8080"),
            "裸值去空白"
        );
        assert_eq!(
            parse_win_proxy_server("http=1.2.3.4:80;https=5.6.7.8:443;ftp=9.9.9.9:21").as_deref(),
            Some("5.6.7.8:443"),
            "分协议优先 https"
        );
        assert_eq!(
            parse_win_proxy_server("http=1.2.3.4:80").as_deref(),
            Some("1.2.3.4:80"),
            "只有 http 段用 http"
        );
        assert!(
            parse_win_proxy_server("ftp=1.2.3.4:21;socks=5.6.7.8:1080").is_none(),
            "只配 ftp/socks 不硬猜"
        );
        assert!(parse_win_proxy_server("").is_none());
        assert!(parse_win_proxy_server("   ").is_none());
        assert!(
            parse_win_proxy_server("https=").is_none(),
            "空段畸形不算命中"
        );
    }

    /// 安装详情取尾不取头：回显/进度剥掉、PS 异常首行只留 iex 后消息、超长保留末尾 600 字。
    #[test]
    fn install_err_tail_strips_echo_and_keeps_tail() {
        // 多行实拍形态：进度行 + 真错误在 stdout 尾部；stderr 首行是整句命令回显
        let stdout = "Installing Claude Code native build latest...\nDownloading...\nFailed to download from https://storage.googleapis.com/x: timeout\n".as_bytes();
        let stderr = "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor 3072; irm https://claude.ai/install.ps1 | iex : Installation failed (exit code 1)\nAt line:1 char:1\n+ FullyQualifiedErrorId : xxx\n".as_bytes();
        let t = install_err_tail(stdout, stderr);
        assert!(t.contains("Failed to download"), "真错误保留: {t}");
        assert!(
            t.contains("Installation failed (exit code 1)"),
            "回显行只留 iex 后消息: {t}"
        );
        assert!(!t.contains("SecurityProtocol"), "命令回显剥掉");
        assert!(!t.contains("Installing Claude Code"), "进度行剥掉");
        assert!(
            t.contains("At line:1"),
            "PS 装饰行保留（取尾语义，不做白名单）"
        );
        // 超长取尾：真错误在末尾 600 字里，且带截断记号
        let long = format!("{}\nREAL ERROR AT TAIL", "x".repeat(2000));
        let t2 = install_err_tail(long.as_bytes(), b"");
        assert!(t2.starts_with('…') && t2.ends_with("REAL ERROR AT TAIL"));
        assert!(
            t2.chars().count() <= 601,
            "…+600 字上限: {}",
            t2.chars().count()
        );
        // 无回显的普通短输出原样、不截
        assert_eq!(install_err_tail(b"boom", b""), "boom");
        // 全空 / 剥完只剩回显 → 占位
        assert_eq!(install_err_tail(b"", b""), "（无输出）");
        assert_eq!(
            install_err_tail(b"Installing Claude Code native build latest...", b""),
            "（无输出）"
        );
    }

    fn test_conv(brain: &str, model: &str, perm: &str) -> Conversation {
        serde_json::from_value(serde_json::json!({
            "id": "c1", "conn_id": "t", "conn_name": "", "site_slug": "s", "site_name": "",
            "task_type": "free", "brain": brain, "model": model, "perm_mode": perm,
            "session_ref": "sess-old", "title": "t", "messages": [], "status": "idle",
            "created_at": 0, "updated_at": 0,
        }))
        .expect("test conversation")
    }

    #[test]
    fn api_effort_switch_resolves_profile_default_without_reading_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let id = "11111111-1111-4111-8111-111111111111";
        let brain = format!("api:{id}");
        let profile = api_profiles::ApiProfile {
            id: id.into(), name: "test".into(), protocol: "anthropic_messages".into(),
            base_url: "https://api.anthropic.com/v1".into(), enabled: true,
            models: vec!["claude-sonnet-5-5".into(), "claude-opus-4-6".into()],
            default_model: "claude-sonnet-5-5".into(), max_output_tokens: 8192,
        };
        std::fs::write(dir.path().join("model-api-profiles.json"), serde_json::to_vec(&vec![profile]).unwrap()).unwrap();
        let mut c = test_conv(&brain, "", "full");
        c.effort = "api:xhigh".into();
        assert_eq!(conversation_effort_after_switch(dir.path(), &c, &brain, "").unwrap(), "api:xhigh");
        assert_eq!(conversation_effort_after_switch(dir.path(), &c, &brain, "claude-opus-4-6").unwrap(), "");
        assert_eq!(conversation_effort_after_switch(dir.path(), &c, "claude", "sonnet").unwrap(), "");
        c.effort = "high".into();
        assert_eq!(conversation_effort_after_switch(dir.path(), &c, &brain, "").unwrap(), "", "hidden legacy API effort must not become active");
    }

    /// ★ 放开 codex 跑远程连接之后，**AI 桥那道确认卡是它仅剩的闸**（闸在 bridge.rs，与厂商无关）。
    /// 所以「codex 的 ask/auto 落 full」这条规则**绝不能**落到 ssh 对话上：
    /// 前端切完厂商会立刻 rebuild_session 自动重跑，静默升档 = 一次「换个模型」的点击
    /// 就在真机上无人确认地跑起来了。这是评审里的 critical，别改回去。
    #[test]
    fn ssh_conv_keeps_ask_auto_when_switching_to_codex() {
        for m in ["ask", "auto"] {
            let mut c = test_conv("claude", "sonnet", m);
            apply_brain_switch(&mut c, "codex", "gpt-5.6", true); // is_ssh
            assert_eq!(
                c.perm_mode, m,
                "ssh 对话切 codex 必须保住 {m}（桥还要靠它弹卡）"
            );
            assert_eq!(c.session_ref, "", "跨厂商仍要抛弃旧 session");
        }
        // 非 ssh 照旧落 full：那里 codex 真的没有任何逐命令闸，ask/auto 名不副实（原行为不变）
        let mut c = test_conv("claude", "sonnet", "auto");
        apply_brain_switch(&mut c, "codex", "gpt-5.6", false);
        assert_eq!(c.perm_mode, "full");
    }

    /// 跨厂商切换：换 brain 清 session_ref（另一家 resume 不了旧会话）+ codex 下 ask/auto 落 full；
    /// 同厂商只换 model，session/perm 原样保留。effort 字段两条路径都不碰。
    #[test]
    fn apply_brain_switch_rules() {
        // claude → codex：清 session、auto 落 full
        let mut c = test_conv("claude", "sonnet", "auto");
        apply_brain_switch(&mut c, "codex", "gpt-5.6-terra", false);
        assert_eq!(c.brain, "codex");
        assert_eq!(c.model, "gpt-5.6-terra");
        assert_eq!(c.session_ref, "", "跨厂商必须抛弃旧 session");
        assert_eq!(c.perm_mode, "full", "codex 无逐命令确认，auto 落 full");

        // claude → codex：plan 档保留（只读语义两家通用）
        let mut c = test_conv("claude", "opus", "plan");
        apply_brain_switch(&mut c, "codex", "", false);
        assert_eq!(c.perm_mode, "plan");

        // codex → claude：ask/auto 不存在被误改的方向，perm 原样
        let mut c = test_conv("codex", "gpt-5.5", "full");
        apply_brain_switch(&mut c, "claude", "sonnet", false);
        assert_eq!(c.session_ref, "");
        assert_eq!(c.perm_mode, "full");

        // claude → grok：grok 有逐命令批准（ACP 权限桥），ask/auto 原样保留
        let mut c = test_conv("claude", "sonnet", "ask");
        apply_brain_switch(&mut c, "grok", "", false);
        assert_eq!(c.brain, "grok");
        assert_eq!(c.session_ref, "");
        assert_eq!(c.perm_mode, "ask");

        // 同厂商换档：session_ref 保留（还是同一条底层会话）
        let mut c = test_conv("claude", "sonnet", "ask");
        apply_brain_switch(&mut c, "claude", "opus", false);
        assert_eq!(c.session_ref, "sess-old");
        assert_eq!(c.perm_mode, "ask");
        assert_eq!(c.model, "opus");
    }

}

/// 读工作目录内的图片为 data URI（webview 读不了本地文件）。附件缩略图（uploads/xx）和
/// 助手消息里提到的项目内图片（如 brand/logo.svg）都走它。扩展名白名单；
/// 前端打开会话会批量调用：async + spawn_blocking，读盘/编码不占主线程；
/// 超过 8MB 不生成（data URI 会膨胀 1/3 常驻前端内存），退回文件卡展示。
#[tauri::command]
async fn read_workdir_image(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    project: String,
    path: String,
) -> Result<String, String> {
    let mime = image_mime(&path).ok_or("不是图片文件")?;
    let (conn, runtime_project) = attachment_runtime(&state, &conn_id, &project, None)?;
    tauri::async_runtime::spawn_blocking(move || {
        let p = resolve_in_workdir(&conn, &runtime_project, &path)?;
        let meta = std::fs::metadata(&p).map_err(|e| format!("读取失败: {e}"))?;
        if !meta.is_file() {
            return Err("不是文件".into());
        }
        if meta.len() > 8_000_000 {
            return Err("图片较大（>8MB），不生成缩略图".into());
        }
        let data = std::fs::read(&p).map_err(|e| format!("读取失败: {e}"))?;
        use base64::Engine as _;
        Ok(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(data)
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 把工作目录内的相对路径解析成绝对路径（给「在文件管理器中显示」用）。同样的合法性约束。
#[tauri::command]
async fn resolve_workdir_file(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    project: String,
    path: String,
) -> Result<String, String> {
    let (conn, runtime_project) = attachment_runtime(&state, &conn_id, &project, None)?;
    tauri::async_runtime::spawn_blocking(move || {
        let p = resolve_in_workdir(&conn, &runtime_project, &path)?;
        if !p.is_file() {
            return Err("文件不存在".into());
        }
        Ok(p.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 列出某 CF 连接工作区下已有的站点项目（<workspace>/projects/* 目录名）。
#[tauri::command]
fn list_cf_projects(state: tauri::State<'_, AppState>, conn_id: String) -> Vec<String> {
    let Ok(conn) = state.conns.get(&conn_id) else {
        return vec![];
    };
    if conn.kind != "cloudflare" {
        return vec![];
    }
    let dir = std::path::Path::new(&conn.skill_dir).join("projects");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut v: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    v.sort();
    v
}

/// 项目是否已经建出可预览/部署的内容（有 index.html / *.html / package.json）。
/// 前端据此在"还只给了方案、没写文件"时把预览/部署置灰，避免点太早。
#[tauri::command]
fn cf_project_ready(state: tauri::State<'_, AppState>, conn_id: String, project: String) -> bool {
    let Ok(conn) = state.conns.get(&conn_id) else {
        return false;
    };
    if conn.kind != "cloudflare" {
        return false;
    }
    let dir = std::path::Path::new(&conn.skill_dir)
        .join("projects")
        .join(cf_project_slug(&project));
    fn has_site(dir: &std::path::Path, depth: u8) -> bool {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return false;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let p = e.path();
            if p.is_file() {
                if name == "index.html" || name == "package.json" || name.ends_with(".html") {
                    return true;
                }
            } else if depth < 3 && p.is_dir() && !name.starts_with('.') && name != "node_modules" {
                if has_site(&p, depth + 1) {
                    return true;
                }
            }
        }
        false
    }
    has_site(&dir, 0)
}

// ---- 本地预览（wrangler pages dev + 预览窗口）----

/// 预览跑在什么上面。
///
/// 静态站**不该逼人装 wrangler**（Windows 上尤其：没 Node 就直接卡死在「25 秒没起来」）——
/// 内置模板本来就是单文件零外链的静态 HTML，内建服务器伺候它是零失真、且瞬间就起。
/// 只有真用到 Workers 运行时/Pages 特性的项目才走 wrangler，见 static_server::needs_wrangler。
enum PreviewProc {
    /// 外部进程：wrangler，或 pilot.json 里自定义的 dev 命令。停的时候整组杀。
    Child(std::process::Child),
    /// 内建静态服务器：是个 task 不是进程，abort 掉即可（listener 随之 drop，端口立刻松手）。
    Builtin(tauri::async_runtime::JoinHandle<()>),
}

struct PreviewHandle {
    proc: PreviewProc,
    #[allow(dead_code)]
    url: String,
    #[allow(dead_code)]
    project: String,
}

const PREVIEW_DEFAULT_PORT: u16 = 8788;

/// 端口上是否真有服务在听。
///
/// ★★ **必须用 connect 探，不能用 bind 探**（第一版就栽在这，实测过）：
/// macOS/BSD 上 `SO_REUSEADDR`（Rust 的 `TcpListener::bind` **默认就设**）允许在别人已经绑了
/// **通配** `*:8788` 的情况下，照样绑上**具体**的 `127.0.0.1:8788` —— 于是 bind 高高兴兴说「空闲」，
/// 而那个端口上真有服务在听。实测：别的 app 占着 `*:8788` 时 `bind(127.0.0.1:8788)` 返回成功、
/// 同时 `connect(127.0.0.1:8788)` 也成功。用 bind 探 = 挑中一个「看着空、其实有人」的端口，
/// 浏览器过去就是别人的页面 —— 正是要修的那个 bug 换了个姿势复发。
///
/// connect 才是对的判据：连得通＝真有人听＝浏览器过去看到的就是他 —— 这正是我们要防的那件事。
/// （没人听时 macOS 立刻 ECONNREFUSED，扫端口不会卡。）
fn port_taken(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok()
}

/// 从 want 起往上找一个没人在听的端口。
///
/// 为什么必须找而不是写死 8788：机器上任何别的东西占着它，预览就会开出**别人的页面**
/// （用户真机上另一个 app 一直听着 8788）。内置模板没有 pilot.json、永远吃默认端口，所以必踩。
fn free_port(want: u16) -> Option<u16> {
    (want..want.saturating_add(64)).find(|p| !port_taken(*p))
}

/// port_taken 的异步版（给 async 命令用：150ms 的阻塞 connect 不该占着 tokio 工作线程）。
async fn port_taken_async(port: u16) -> bool {
    tokio::time::timeout(
        Duration::from_millis(150),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false)
}

/// free_port 的异步版：最多 64 次 150ms 的探测，放阻塞线程池做。
async fn free_port_async(want: u16) -> Option<u16> {
    tauri::async_runtime::spawn_blocking(move || free_port(want)).await.ok().flatten()
}

/// 等预览服务真的能连上。替掉写死的 2.5s：机器慢就白屏、起不来也照样开窗，两头不讨好。
/// 起得快就早开（常见情况比 2.5s 还快），起不来就明说。
async fn wait_preview(port: u16, secs: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    false
}

/// `wrangler pages dev` 不给 compatibility_date 时会**默认成「今天」**，而它随附的 workerd 只支持到
/// 某个更早的日期 → Workers runtime 直接起不来 → **预览窗一片空白**（实测：wrangler 4.107 的 workerd
/// 上限是 2026-07-08，默认的 today=2026-07-17 就炸，日志里写着 "The Workers runtime failed to start"）。
/// 项目自己没声明时给个保守日期兜底：静态站根本用不到新运行时特性，而「给太新」的失败是**静默白屏**、
/// 「给太旧」顶多少几个新 API —— 所以宁可偏旧。实测 2024-01-01 正常返回 200。
const FALLBACK_COMPAT_DATE: &str = "2024-01-01";

/// 项目自己声明了 compatibility_date？（wrangler.toml / .json / .jsonc）
/// ★ 只在没声明时才给 `--compatibility-date`：**CLI 参数会盖过配置文件**，硬给会把人家项目
/// 声明的日期顶掉，Functions/D1 那些依赖新运行时的站会被我们悄悄降级。
fn declares_compat_date(dir: &std::path::Path) -> bool {
    ["wrangler.toml", "wrangler.json", "wrangler.jsonc"]
        .iter()
        .any(|f| {
            std::fs::read_to_string(dir.join(f))
                .map(|s| s.contains("compatibility_date"))
                .unwrap_or(false)
        })
}

/// 预览怎么跑。
enum PreviewPlan {
    /// Pilot 自己拼的 wrangler 命令（项目用到 Workers/Pages 特性时）。这是我们自己的命令，可以给它 CF token。
    Wrangler(String, u16),
    /// pilot.json 里自定义的 dev 命令。pilot.json 是 AI 会写的文件——这条命令**执行前必须经人确认**，
    /// 而且**不给 CF token**（否则 AI 写一行 `"dev": "curl -d $CLOUDFLARE_API_TOKEN …"` 就把 token 带走了）。
    Custom(String, u16),
    /// 内建静态服务器直接伺候这个目录 —— **不需要 wrangler、不需要 Node**。
    Static(std::path::PathBuf, u16),
}

/// cf_preview_start 的结果：要么预览已经开了，要么得先让人确认 pilot.json 里那条自定义命令。
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PreviewStart {
    Started { url: String },
    /// 把原样的命令给前端弹确认框；用户点了「运行」前端再带着这条命令（confirmed_command）重新调用。
    Confirm { command: String },
}

/// 用户已确认过的自定义预览命令记在 Pilot 数据目录（不能记在项目里：项目目录 AI 能写，
/// 写一条「已确认」等于绕过确认）。键 = sha256(连接 id + 项目 + 命令原文)，换一条命令就再问一次。
fn preview_confirm_key(conn_id: &str, project: &str, command: &str) -> String {
    node_boot::sha256_hex(format!("{conn_id}\n{project}\n{command}").as_bytes())
}

fn preview_confirmed_file(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("preview-confirmed.json")
}

fn preview_cmd_confirmed(data_dir: &std::path::Path, key: &str) -> bool {
    let keys: Vec<String> = store_file::read_or_quarantine(&preview_confirmed_file(data_dir));
    keys.iter().any(|k| k == key)
}

fn remember_preview_cmd(data_dir: &std::path::Path, key: &str) -> Result<(), String> {
    let file = preview_confirmed_file(data_dir);
    let mut keys: Vec<String> = store_file::read_or_quarantine(&file);
    if keys.iter().any(|k| k == key) {
        return Ok(());
    }
    keys.push(key.to_string());
    let raw = serde_json::to_vec(&keys).map_err(|e| e.to_string())?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, raw).map_err(|e| format!("记录已确认命令失败: {e}"))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("记录已确认命令失败: {e}"))
}

/// 起外部预览进程（wrangler 或自定义 dev 命令）。`env` 里只放这条命令该拿到的变量。
fn spawn_preview_cmd(shell_cmd: &str, dir: &str, env: &[(&str, &str)]) -> Result<std::process::Child, String> {
    #[cfg(windows)]
    let mut c = {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C").arg(shell_cmd);
        c
    };
    #[cfg(not(windows))]
    let mut c = {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(shell_cmd);
        c
    };
    c.current_dir(dir);
    for (k, v) in env {
        c.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0); // 整组，停预览时连 node 子进程一起杀
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.spawn().map_err(|e| format!("启动预览失败: {e}\n{}", wrangler_hint()))
}

/// 预览方案决策（两个预览入口共用）。三条路，优先级从高到低：
///
/// - **自定义 dev 命令**（pilot.json 声明）：永远听用户的。端口写死在命令里、只能用它声明的那个；
///   被占就直说，绝不开个窗让用户对着别人的页面猜。
/// - **静态站**：内建服务器伺候，端口由我们挑（挑空的），不需要 wrangler/Node，也没有冷启。
/// - **用到 Workers/Pages 特性**：只能 wrangler。端口同样我们挑；顺带兜住 compat date（见上）。
fn preview_plan(
    dir: &std::path::Path,
    dev_cmd: Option<String>,
    out: &str,
    port: u16,
) -> Result<PreviewPlan, String> {
    if let Some(d) = dev_cmd {
        if port_taken(port) {
            return Err(format!(
                "预览端口 {port} 已被别的程序占用。项目 pilot.json 里的 dev 命令写死了这个端口，\
Pilot 改不了 —— 先关掉占用它的程序，或在 pilot.json 里换一个 \"port\"（并同步改 dev 命令）。"
            ));
        }
        return Ok(PreviewPlan::Custom(d, port));
    }
    let p = free_port(port)
        .ok_or_else(|| format!("找不到空闲的预览端口（{port} 往上 64 个都被占着）。"))?;
    let serve_dir = if out.is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(out)
    };
    // ★ 静态站不该逼人装 wrangler。内置模板全是单文件零外链的静态 HTML，内建服务器伺候它
    //   零失真、且瞬间就起；而 wrangler 要 Node、要一大包，Windows 上没装就直接卡死在
    //   「25 秒没起来」（用户原话：「一定要安装 wrangler 吗」）。
    //   反过来，真用到 Workers 运行时/Pages 特性的**故意不降级**——静态服务器给不出正确结果，
    //   给个「能打开但和线上不一样」的假象比明说要装 wrangler 更坏。
    //   项目根和实际服务目录都要看：functions/ 与 wrangler.* 在根，_headers/_redirects 可能在产物里。
    if !static_server::needs_wrangler(dir) && !static_server::needs_wrangler(&serve_dir) {
        return Ok(PreviewPlan::Static(serve_dir, p));
    }
    let target = if out.is_empty() { "." } else { out };
    let compat = if declares_compat_date(dir) {
        String::new()
    } else {
        format!(" --compatibility-date={FALLBACK_COMPAT_DATE}")
    };
    Ok(PreviewPlan::Wrangler(
        format!("wrangler pages dev {target} --ip 127.0.0.1 --port {p}{compat}"),
        p,
    ))
}

/// 从预览 URL 里取端口。
fn url_port(url: &str) -> Option<u16> {
    url.rsplit(':').next()?.trim_end_matches('/').parse().ok()
}

/// 杀完等端口真的松手。
///
/// 为什么必须等：真正绑端口的是 wrangler 派生的 **workerd**，而 `stop_preview` 杀完进程组只
/// `wait()` 了直接子进程（sh/wrangler）—— workerd 关 socket 是异步的。不等的话，紧接着的探测会把
/// 这个「正在退场」的端口判成「被占」→ 让到下一个 → 端口一路上爬（8789→8790→8791…）。
async fn wait_port_released(port: u16, ms: u64) {
    let deadline = std::time::Instant::now() + Duration::from_millis(ms);
    while std::time::Instant::now() < deadline {
        if !port_taken_async(port).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

/// 同一个东西、预览还活着、窗还在 → 别重启，直接复用（调用方负责 show+focus）。
/// `wrangler pages dev` 本来就盯着文件变化，重启纯属让用户白等一次启动。
async fn reusable_preview(app: &AppHandle, state: &AppState, project: &str) -> Option<String> {
    // 锁里只抄出 url，探端口要 await，别攥着锁。
    let url = {
        let g = state.preview.lock().unwrap();
        let h = g.as_ref()?;
        if h.project != project {
            return None;
        }
        h.url.clone()
    };
    let port = url_port(&url)?;
    if !port_taken_async(port).await {
        return None; // 进程已经死了 → 老老实实重起
    }
    app.get_webview_window("preview")?; // 窗关了（关窗会 stop_preview）→ 重起
    Some(url)
}

/// preview_plan 的异步包装：里面要探端口（最多 64 次阻塞 connect），放阻塞线程池。
async fn preview_plan_async(dir: std::path::PathBuf, dev_cmd: Option<String>, out: String, port: u16) -> Result<PreviewPlan, String> {
    tauri::async_runtime::spawn_blocking(move || preview_plan(&dir, dev_cmd, &out, port))
        .await
        .map_err(|e| e.to_string())?
}

/// pilot.json → (dev 命令, 产物目录, 预览端口)。缺省 dev=None、out=""、port=8788。
/// 自定义 dev 命令监听的端口必须在 pilot.json 的 "port" 里写对，否则预览窗打不开。
fn read_pilot_json(dir: &std::path::Path) -> (Option<String>, String, u16) {
    let Ok(raw) = std::fs::read(dir.join("pilot.json")) else {
        return (None, String::new(), PREVIEW_DEFAULT_PORT);
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return (None, String::new(), PREVIEW_DEFAULT_PORT);
    };
    let dev = v
        .get("dev")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let out = v
        .get("out")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let port = v
        .get("port")
        .and_then(|x| x.as_u64())
        .filter(|p| *p >= 1 && *p <= 65535)
        .map(|p| p as u16)
        .unwrap_or(PREVIEW_DEFAULT_PORT);
    (dev, out, port)
}

/// 停掉当前预览。外部进程要整树杀并回收（避免僵尸）；内建服务器 abort 掉即可。
/// 杀进程 + wait 是阻塞调用，丢到阻塞线程池去做——调用方（async 命令 / 窗口事件）不等它；
/// 真要等端口松手的地方用 wait_port_released 探端口。
fn stop_preview(state: &AppState) {
    let Some(h) = state.preview.lock().unwrap().take() else {
        return;
    };
    match h.proc {
        PreviewProc::Builtin(task) => task.abort(), // listener 随之 drop，端口立刻松手
        PreviewProc::Child(child) => {
            tauri::async_runtime::spawn_blocking(move || kill_preview_tree(child));
        }
    }
}

fn kill_preview_tree(mut child: std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &format!("-{pid}")])
            .status();
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// 输入框/选中文字的右键菜单。**必须是原生菜单，不能前端自己画。**
///
/// 病根：macOS 14 起，程序**读**剪贴板（`navigator.clipboard.readText`）会弹一个系统「Paste」
/// 确认按钮 —— 用户点完我们的「粘贴」还得再点一次系统的 Paste，看着就像坏了。
/// 而且这个提示**用 JS 绕不过去**：系统只看到「某程序要读剪贴板」，看不到用户刚点过菜单，
/// 那正是它要防的事。（剪切/复制是**写**，所以一直没事——只有粘贴犯规。）
///
/// 原生 `PredefinedMenuItem::paste` 由系统自己执行，两个平台都不弹：
/// - macOS：走 responder chain 的 `paste:`，webview 自己粘贴。
/// - Windows：muda 模拟一次真实的 Ctrl+V 按键，WebView2 当普通按键处理。
///
/// 白送的好处：启用态由系统按当前选区/剪贴板自动管（macOS 的 `validateMenuItem:`）——
/// 没选中就自动灰掉「剪切/复制」，剪贴板空就灰掉「粘贴」，不用我们在前端猜。
#[tauri::command]
fn show_edit_menu(app: AppHandle, window: tauri::Window, editable: bool) -> Result<(), String> {
    use tauri::menu::{ContextMenu, MenuBuilder, PredefinedMenuItem};
    let mut b = MenuBuilder::new(&app);
    if editable {
        let cut = PredefinedMenuItem::cut(&app, Some("剪切")).map_err(|e| e.to_string())?;
        let copy = PredefinedMenuItem::copy(&app, Some("复制")).map_err(|e| e.to_string())?;
        let paste = PredefinedMenuItem::paste(&app, Some("粘贴")).map_err(|e| e.to_string())?;
        let all = PredefinedMenuItem::select_all(&app, Some("全选")).map_err(|e| e.to_string())?;
        b = b.item(&cut).item(&copy).item(&paste).separator().item(&all);
        b.build()
            .map_err(|e| e.to_string())?
            .popup(window)
            .map_err(|e| e.to_string())
    } else {
        // 非输入框：只给「复制」（选中的消息文字）
        let copy = PredefinedMenuItem::copy(&app, Some("复制")).map_err(|e| e.to_string())?;
        b = b.item(&copy);
        b.build()
            .map_err(|e| e.to_string())?
            .popup(window)
            .map_err(|e| e.to_string())
    }
}

/// 在新窗口里打开一个连接（几台机器 / 几个站点并排看）。同一连接已有窗口就把它拎到前面。
///
/// 窗口之间只是各自一份前端：连接表、对话、SSH 会话、权限待批都在 Rust 侧的同一份 AppState 里，
/// 所以两个窗口看到的是同一份数据，不会分叉。
///
/// **连接 id 走窗口 label（`conn-<id>`），不走 URL**：`WebviewUrl::App` 只有在路径**恰好**是
/// "index.html" 时才直接用根 URL，带上 `?conn=…` 就会 join 成 `/index.html?conn=…` ——
/// 而 SvelteKit 的 dev server 只认 `/`、不认 `/index.html`，那样开出来是一片白。
/// label 本来就是现成的（上面判重就用它），前端读 `getCurrentWindow().label` 即可。
#[tauri::command]
async fn open_conn_window(
    app: AppHandle,
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
    conn_id: String,
) -> Result<(), String> {
    // 本地对话 / 站点是虚拟连接，不在 connections.json 里
    let title = match conn_id.as_str() {
        "__workspace__" => "本地对话".to_string(),
        "__sites__" => "站点".to_string(),
        _ => state.conns.get(&conn_id)?.name,
    };
    // 标签只允许字母数字和 -/:_ —— uuid 和 __sites__ 都合规。
    let label = format!("conn-{conn_id}");
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    // 跟随发起「新窗口打开」的那扇窗口，而不是每次退回固定 1024×700。
    // inner_size 是物理像素，Builder 收的是逻辑像素；必须按源窗口缩放系数换算，
    // 否则 Retina / Windows 缩放屏上新窗口会放大一倍或明显变小。
    let source_size = match (window.inner_size(), window.scale_factor()) {
        (Ok(size), Ok(scale)) => Some(size.to_logical::<f64>(scale)),
        _ => None,
    };
    let source_maximized = window.is_maximized().unwrap_or(false);
    let mut b = tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        // 必须原样是 "index.html"：这条路径和主窗口走的是同一个分支（直接用根 URL）。
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title(&title)
    .maximized(source_maximized)
    .min_inner_size(820.0, 560.0)
    // Windows 上 Tauri 原生文件拖放会截走 WebView2 的 HTML5 drop；关闭后输入框可直接接收 File。
    .disable_drag_drop_handler();
    b = if let Some(size) = source_size {
        b.inner_size(size.width, size.height)
    } else {
        b.inner_size(1024.0, 700.0)
    };
    // 主窗口在 tauri.conf 里是 Overlay + 隐藏标题（前端自己画标题栏），新窗口得对齐，
    // 否则同一套前端布局会和系统标题栏叠在一起。这两个设置只有 macOS 有。
    #[cfg(target_os = "macos")]
    {
        b = b
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }
    let w = b.build().map_err(|e| format!("打开新窗口失败: {e}"))?;
    #[cfg(target_os = "windows")]
    style_titlebar_windows(&w);
    let _ = w.set_focus();
    Ok(())
}

/// 预览窗的标题：带上正在预览的是谁（一次只跑一个预览，窗会被复用 → 标题必须跟着换，
/// 否则复用后还挂着上一个的名字，比不写还糟）。
fn preview_title(name: &str) -> String {
    match name.trim() {
        "" => "本地预览".into(),
        n => format!("{n} · 本地预览"),
    }
}

fn open_preview_window(app: &AppHandle, url: &str, name: &str) -> Result<(), String> {
    let title = preview_title(name);
    if let Some(w) = app.get_webview_window("preview") {
        let _ = w.set_title(&title);
        let _ = w.eval(&format!("window.location.replace('{url}')"));
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let parsed = url.parse().map_err(|e| format!("预览地址解析失败: {e}"))?;
    // mut 只有 macOS 分支用得上
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut b =
        tauri::WebviewWindowBuilder::new(app, "preview", tauri::WebviewUrl::External(parsed))
            .title(&title)
            .inner_size(1024.0, 728.0);
    #[cfg(target_os = "macos")]
    {
        // 这里**故意不用主窗口那套 Overlay**。Overlay = titlebarTransparent + fullSizeContentView，
        // 内容会顶到标题栏底下——主窗口无所谓（前端自己画标题栏、给红绿灯留了位），可预览窗装的是
        // **别人的页面**：站点自己的导航会被红绿灯压住，预览就不再是站点真实的样子了。
        // Transparent = titlebarTransparent + fullSizeContentView(false)：栏透明（透出下面设的窗底色）、
        // 内容仍从栏下开始、标题文字还在（模块名要显示）。
        // 透明栏透出的是 NSWindow.backgroundColor，所以必须显式给白 —— 不给就是系统灰，
        // 也就是现在这条突兀的灰带；白底能和绝大多数页面接上（深色页会留一条白，是这条路的代价：
        // 想让栏跟着页面变色，就得从外部页面取色回传，而外部 origin 默认没有 IPC）。
        b = b
            .title_bar_style(tauri::TitleBarStyle::Transparent)
            .background_color(tauri::window::Color(255, 255, 255, 255));
    }
    b.build().map_err(|e| format!("打开预览窗口失败: {e}"))?;
    Ok(())
}

/// 起内建静态服务器。**先 bind 再返回**：端口到底拿没拿到，必须在开预览窗之前就知道 ——
/// 而不是像外部进程那条路，起不来要等 25 秒超时才发现。
async fn start_builtin(dir: std::path::PathBuf, port: u16) -> Result<PreviewProc, String> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("预览端口 {port} 绑不上: {e}"))?;
    Ok(PreviewProc::Builtin(tauri::async_runtime::spawn(
        static_server::serve(dir, listener),
    )))
}

/// 启动本地预览：静态站用内建服务器（不需要 wrangler），有 Workers/Pages 特性的才跑 wrangler；
/// pilot.json 自定义了 dev 就跑它——但那是 AI 会写的文件，所以**第一次见到这条命令要先让人确认**
/// （返回 Confirm，前端弹框，用户点运行后带着 confirmed_command 再来），且**不把 CF token 给它**。
/// 然后开预览窗。
#[tauri::command]
async fn cf_preview_start(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    conn_id: String,
    project: String,
    confirmed_command: Option<String>,
) -> Result<PreviewStart, String> {
    let conn = state.conns.get(&conn_id)?;
    if conn.kind != "cloudflare" {
        return Err("这个连接不是 Cloudflare".into());
    }
    let dir = resolve_work_dir(&conn, &project)?;
    // 同一个项目、预览还活着、窗还在 → 直接聚焦，不重启（dev server 本来就热更新）。
    if let Some(u) = reusable_preview(&app, &state, &project).await {
        open_preview_window(&app, &u, &project)?;
        return Ok(PreviewStart::Started { url: u });
    }
    let (dev_cmd, out, port) = read_pilot_json(std::path::Path::new(&dir));
    // 先定方案再停旧预览：要是还得问人，别先把正在跑的预览掐了。
    let plan = preview_plan_async(std::path::PathBuf::from(&dir), dev_cmd, out, port).await?;
    if let PreviewPlan::Custom(cmd, _) = &plan {
        let key = preview_confirm_key(&conn.id, &project, cmd);
        // 确认必须绑定命令原文：弹框到点「运行」之间 pilot.json 可能又被改了，改了就再问。
        if confirmed_command.as_deref() == Some(cmd.as_str()) {
            remember_preview_cmd(&state.data_dir, &key)?;
        } else if !preview_cmd_confirmed(&state.data_dir, &key) {
            return Ok(PreviewStart::Confirm { command: cmd.clone() });
        }
    }
    // 一次只跑一个预览。先记下旧端口：停完要等它**真的**松手，否则紧接着的探测会把「正在退场」的
    // 端口当成「被占」，端口就一路往上爬。
    let old_port = state
        .preview
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|h| url_port(&h.url));
    stop_preview(&state);
    if let Some(p) = old_port {
        wait_port_released(p, 2500).await;
    }
    let (proc, port, external) = match plan {
        PreviewPlan::Static(sd, p) => (start_builtin(sd, p).await?, p, false),
        PreviewPlan::Wrangler(shell_cmd, p) => {
            // 只有我们自己拼的 wrangler 命令才拿得到 token。
            let token = keychain::get_key_async(&conn.id).await?;
            let mut env: Vec<(&str, &str)> = vec![("CLOUDFLARE_API_TOKEN", token.as_str())];
            if !conn.account_id.is_empty() {
                env.push(("CLOUDFLARE_ACCOUNT_ID", conn.account_id.as_str()));
            }
            let child = spawn_preview_cmd(&shell_cmd, &dir, &env)?;
            (PreviewProc::Child(child), p, true)
        }
        PreviewPlan::Custom(shell_cmd, p) => {
            // 自定义命令：不注入任何凭据（账号 id 不是秘密，给它无妨）。
            let mut env: Vec<(&str, &str)> = Vec::new();
            if !conn.account_id.is_empty() {
                env.push(("CLOUDFLARE_ACCOUNT_ID", conn.account_id.as_str()));
            }
            let child = spawn_preview_cmd(&shell_cmd, &dir, &env)?;
            (PreviewProc::Child(child), p, true)
        }
    };
    let url = format!("http://127.0.0.1:{port}");
    let title_name = project.clone(); // project 下一行就被搬进 handle 了，标题要用得先留一份
    *state.preview.lock().unwrap() = Some(PreviewHandle {
        proc,
        url: url.clone(),
        project,
    });
    // 外部进程才需要等（内建服务器在 bind 成功那刻就已经在听了）。
    if external && !wait_preview(port, 25).await {
        stop_preview(&state);
        return Err(format!(
            "预览没能在 25 秒内起来（端口 {port}）。\n{}",
            wrangler_hint()
        ));
    }
    open_preview_window(&app, &url, &title_name)?;
    Ok(PreviewStart::Started { url })
}

/// wrangler 起不来时的提示。**只在真需要 wrangler 时才会走到这里**（静态站根本不碰它），
/// 所以这里要说清「为什么这个项目躲不掉」，而不是笼统甩一句「装 wrangler」。
fn wrangler_hint() -> String {
    "这个项目用到了 Workers/Pages 特性（functions/、_worker.js、_headers、_redirects 或 wrangler 配置），\
只有 wrangler 能正确预览它 —— 纯静态站 Pilot 会用内建服务器，不需要装任何东西。\n\
装法：先装 Node.js（nodejs.org），再执行 npm i -g wrangler。\n\
若项目 pilot.json 自定义了 dev 命令，也看看它是不是真监听这个端口。"
        .to_string()
}

/// 站点的本地预览：用内建静态服务器伺候构建产物（和上线的是同一份文件），开预览窗。
/// 产物改了不用重启：服务器每次请求都现读文件，重新构建后刷新即可。
#[tauri::command]
async fn site_preview_start(app: AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let code = site_git::code_dir(&site, state.conns.get(&site.gh_conn_id).ok().as_ref())?;
    // GitHub 来源在 <站点数据>/deploy 工作树里构建
    let deploy = state.sites.data_dir(&id).join("deploy");
    let root = if site.source == "github" && deploy.is_dir() { deploy } else { code };
    let out = site_make::out_dir(&site.app_dir(&root), &site.build.out_dir);
    if static_server::built_at(&out).is_none() {
        return Err("还没有构建产物：先点「构建并检查」".into());
    }
    preview_dir(&app, &state, &format!("site:{id}"), out, &site.name, "").await
}

/// 用内建静态服务器预览一个目录，打开预览窗（`page`：要打开的路径，比如 `/a/b.html`；空＝首页）。
/// 同一个 key 的服务还在就直接复用。
pub(crate) async fn preview_dir(app: &AppHandle, state: &AppState, key: &str, dir: std::path::PathBuf, title: &str, page: &str) -> Result<String, String> {
    if let Some(u) = reusable_preview(app, state, key).await {
        let full = format!("{u}{page}");
        open_preview_window(app, &full, title)?;
        return Ok(full);
    }
    let old_port = state.preview.lock().unwrap().as_ref().and_then(|h| url_port(&h.url));
    stop_preview(state);
    if let Some(p) = old_port {
        wait_port_released(p, 2500).await;
    }
    let port = free_port_async(PREVIEW_DEFAULT_PORT).await.ok_or("找不到空闲的预览端口")?;
    let proc = start_builtin(dir, port).await?;
    let url = format!("http://127.0.0.1:{port}");
    *state.preview.lock().unwrap() = Some(PreviewHandle { proc, url: url.clone(), project: key.to_string() });
    let full = format!("{url}{page}");
    open_preview_window(app, &full, title)?;
    Ok(full)
}

#[tauri::command]
fn cf_preview_stop(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    stop_preview(&state);
    if let Some(w) = app.get_webview_window("preview") {
        let _ = w.close();
    }
    Ok(())
}

/// 预览一个模板：在模板目录里跑 wrangler pages dev + 开预览窗，看它真实的样子。
#[tauri::command]
async fn cf_preview_template(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    slug: String,
) -> Result<String, String> {
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("非法模板名".into());
    }
    let dir = state.data_dir.join("templates").join(&slug);
    if !dir.is_dir() {
        return Err("模板不存在".into());
    }
    // 标题给人看的是模板名（「企业 SaaS」），不是 slug（「saas」）。
    let title_name = cf_templates::display_name(&state.data_dir.join("templates"), &slug);
    // 同一个模板、预览还活着、窗还在 → 直接聚焦，不重启。
    let label = format!("template:{slug}");
    if let Some(u) = reusable_preview(&app, &state, &label).await {
        open_preview_window(&app, &u, &title_name)?;
        return Ok(u);
    }
    let (dev_cmd, out, port) = read_pilot_json(&dir);
    // 模板不属于任何连接，预览**不给任何 token**（本地 wrangler pages dev 用不着）。
    // 以前随手拿第一个 CF 连接的 token 塞进去——模板里的 pilot.json 是 AI 写的，等于把 token 交给它。
    let plan = preview_plan_async(dir.clone(), dev_cmd, out, port).await?;
    if let PreviewPlan::Custom(cmd, _) = &plan {
        return Err(format!(
            "这个模板的 pilot.json 自定义了预览命令（{cmd}）。模板预览不执行自定义命令——先用这个模板建项目，再在项目里预览。"
        ));
    }
    let old_port = state
        .preview
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|h| url_port(&h.url));
    stop_preview(&state);
    if let Some(p) = old_port {
        wait_port_released(p, 2500).await; // 见 wait_port_released：不等就会端口上爬
    }
    // 随附模板全是「单文件、内联 CSS、零外部资源」的静态 HTML（这是库自己的硬契约，有测试钉着），
    // 所以这里几乎总是走内建服务器：**不需要 wrangler、不需要 Node，也没有 25 秒冷启**。
    // （用户存的模板可能带 functions/ 之类，那种才落回 wrangler —— preview_plan 会认出来。）
    let (proc, port, external) = match plan {
        PreviewPlan::Static(sd, p) => (start_builtin(sd, p).await?, p, false),
        PreviewPlan::Wrangler(shell_cmd, p) => {
            let child = spawn_preview_cmd(&shell_cmd, &dir.to_string_lossy(), &[])?;
            (PreviewProc::Child(child), p, true)
        }
        PreviewPlan::Custom(..) => unreachable!("上面已经拒掉"),
    };
    let url = format!("http://127.0.0.1:{port}");
    *state.preview.lock().unwrap() = Some(PreviewHandle {
        proc,
        url: url.clone(),
        project: label, // 与上面 reusable_preview 查的是同一个标签，别让它俩各写各的
    });
    if external && !wait_preview(port, 25).await {
        stop_preview(&state);
        return Err(format!(
            "模板预览没能在 25 秒内起来（端口 {port}）。\n{}",
            wrangler_hint()
        ));
    }
    open_preview_window(&app, &url, &title_name)?;
    Ok(url)
}

// ---- 模板库 ----

#[tauri::command]
fn list_templates(state: tauri::State<'_, AppState>) -> Vec<cf_templates::Template> {
    // 不只在启动时种内置模板：升级后用户点击模板库刷新，也应立即看到新增模板。
    let _ = cf_templates::ensure_builtin(&state.data_dir);
    cf_templates::list(&state.data_dir.join("templates"))
}

/// 读模板入口 HTML（模板卡用 iframe srcdoc 展示真实样子）。找不到返回空串；过大截断。
#[tauri::command]
fn template_index_html(state: tauri::State<'_, AppState>, slug: String) -> String {
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return String::new();
    }
    let dir = state.data_dir.join("templates").join(&slug);
    let mut path = dir.join("index.html");
    if !path.is_file() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            if let Some(h) = rd
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().and_then(|x| x.to_str()) == Some("html"))
            {
                path = h;
            }
        }
    }
    match std::fs::read_to_string(&path) {
        Ok(s) if s.len() <= 900_000 => s,
        Ok(s) => s.chars().take(400_000).collect(),
        Err(_) => String::new(),
    }
}

/// 把某 CF 项目存成模板（沉淀）。
#[tauri::command]
async fn save_as_template(
    state: tauri::State<'_, AppState>,
    conn_id: String,
    project: String,
    name: String,
    desc: String,
) -> Result<cf_templates::Template, String> {
    let conn = state.conns.get(&conn_id)?;
    if conn.kind != "cloudflare" {
        return Err("只有 Cloudflare 站点项目能存为模板".into());
    }
    let src = resolve_work_dir(&conn, &project)?;
    let tdir = state.data_dir.join("templates");
    tauri::async_runtime::spawn_blocking(move || {
        cf_templates::save(&tdir, &name, &desc, std::path::Path::new(&src))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn delete_template(state: tauri::State<'_, AppState>, slug: String) -> Result<(), String> {
    cf_templates::delete(&state.data_dir.join("templates"), &slug)
}

/// 引用模板到某 CF 连接下的新项目（拷贝模板文件，之后在对话里定制）。
#[tauri::command]
async fn use_template(
    state: tauri::State<'_, AppState>,
    slug: String,
    conn_id: String,
    project: String,
) -> Result<(), String> {
    let conn = state.conns.get(&conn_id)?;
    if conn.kind != "cloudflare" {
        return Err("请先切到一个 Cloudflare 连接再用模板建站".into());
    }
    let dest = resolve_work_dir(&conn, &project)?;
    let tdir = state.data_dir.join("templates");
    tauri::async_runtime::spawn_blocking(move || {
        cf_templates::instantiate(&tdir, &slug, std::path::Path::new(&dest))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 手动「重新检测」：先重新导入登录 shell 的 PATH（装完 CLI 后新增的目录也能认到），再探测。
/// 自动轮询仍用轻量的 detect_brains（不每 6s 起登录 shell）。
#[tauri::command]
async fn redetect_brains() -> Result<brains::BrainsInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        path_env::fix();
        apply_system_proxy_to_process();
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(brains::detect().await)
}

/// GUI 进程没有终端里的 HTTP(S)_PROXY 环境变量；从系统代理配置补一份给安装子进程，
/// 让脚本内部的 curl / node 下载步骤也走代理（VPN 用户「浏览器能开 claude.ai 但安装失败」
/// 的主因之一）。用户环境已有代理变量时不覆盖；读不到系统代理就什么都不注入。
fn system_proxy_env() -> Vec<(String, String)> {
    for k in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if std::env::var_os(k).is_some() {
            return vec![];
        }
    }
    let Some(mut u) = system_proxy_url() else {
        return vec![];
    };
    if !u.contains("://") {
        u = format!("http://{u}");
    }
    ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"]
        .iter()
        .map(|k| (k.to_string(), u.clone()))
        .collect()
}

/// 解析 WinINET ProxyServer 注册表值（纯函数，单测覆盖；mac 上也编译便于审读/测试）：
/// 单一 "host:port" 直接用；按协议分段 "http=…;https=…;ftp=…" 时优先 https 再 http；
/// 只配了 ftp=/socks= 等不硬猜；空/空白 → None。
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_win_proxy_server(server: &str) -> Option<String> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    if server.contains('=') {
        for want in ["https=", "http="] {
            if let Some(seg) = server.split(';').find_map(|s| s.trim().strip_prefix(want)) {
                let seg = seg.trim();
                if !seg.is_empty() {
                    return Some(seg.to_string());
                }
            }
        }
        return None; // 只配了 ftp=/socks= 等，不硬猜
    }
    Some(server.to_string())
}

/// Windows：WinINET 用户级代理（ProxyEnable=1 时 ProxyServer 形如 "127.0.0.1:7890"，
/// 或按协议分段 "http=…;https=…"）。
#[cfg(target_os = "windows")]
fn system_proxy_url() -> Option<String> {
    let q = |name: &str| -> Option<String> {
        let mut c = std::process::Command::new("reg");
        c.args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            name,
        ]);
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let out = c.output().ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        s.lines()
            .find(|l| l.trim_start().starts_with(name))
            .and_then(|l| l.split_whitespace().last().map(str::to_string))
    };
    let enabled = q("ProxyEnable")
        .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0)
        != 0;
    if !enabled {
        return None;
    }
    parse_win_proxy_server(&q("ProxyServer")?)
}

/// macOS：scutil --proxy（HTTPSEnable/HTTPSProxy/HTTPSPort，HTTP 同构；curl 不认系统
/// 代理只认环境变量，这里读出来转成环境变量）。
#[cfg(target_os = "macos")]
fn system_proxy_url() -> Option<String> {
    let out = std::process::Command::new("scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let get = |k: &str| -> Option<String> {
        s.lines()
            .find(|l| l.trim_start().starts_with(k))
            .and_then(|l| l.splitn(2, ':').nth(1).map(|v| v.trim().to_string()))
    };
    for (e, h, p) in [
        ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
        ("HTTPEnable", "HTTPProxy", "HTTPPort"),
    ] {
        if get(e).as_deref() == Some("1") {
            if let (Some(host), Some(port)) = (get(h), get(p)) {
                if !host.is_empty() && !port.is_empty() {
                    return Some(format!("{host}:{port}"));
                }
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn system_proxy_url() -> Option<String> {
    None
}

/// 安装失败时抽「脚本自己说的话」：优先 stdout 末尾的有效行（官方脚本把失败原因写在
/// 这里），stderr 只留异常首行并剥掉 PowerShell 的命令回显前缀（"… | iex : 消息"）。
#[cfg(test)]
fn install_err_brief(stdout: &[u8], stderr: &[u8]) -> String {
    let sout = String::from_utf8_lossy(stdout);
    let mut parts: Vec<String> = vec![];
    if let Some(l) = sout.lines().rev().map(str::trim).find(|l| !l.is_empty()) {
        parts.push(l.to_string());
    }
    let serr = String::from_utf8_lossy(stderr);
    if let Some(l) = serr.lines().map(str::trim).find(|l| {
        !l.is_empty()
            && !l.starts_with('+')
            && !l.starts_with('~')
            && !l.starts_with("At line")
            && !l.starts_with("CategoryInfo")
            && !l.starts_with("FullyQualifiedErrorId")
    }) {
        let l = l.rfind("| iex : ").map(|i| &l[i + 8..]).unwrap_or(l).trim();
        if !l.is_empty() && !parts.iter().any(|p| p == l) {
            parts.push(l.to_string());
        }
    }
    if parts.is_empty() {
        parts.push("未知错误".into());
    }
    parts.join("；").chars().take(220).collect()
}

/// 安装失败详情（取尾不取头，纯函数）：合并 stdout+stderr，剥掉我们自己的命令回显与进度行
///（PowerShell 异常首行整句复读命令时只留 "| iex : " 之后的真实消息），保留末尾 ~600 字——
/// PowerShell/npm 的真正异常都在输出末尾，取头只能看到回显、真错误被截掉。
fn install_err_tail(stdout: &[u8], stderr: &[u8]) -> String {
    const MAX: usize = 600;
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    let kept: Vec<String> = combined
        .lines()
        .filter_map(|raw| {
            let l = raw.trim();
            if l.is_empty() || l.starts_with("Installing Claude Code") {
                return None; // 我们/脚本的进度行
            }
            if l.contains("SecurityProtocol") || l.contains("install.ps1 | iex") {
                // 命令回显（PS 异常首行会整句复读命令）：只留 "| iex : " 之后的真实消息
                return l
                    .rfind("| iex : ")
                    .map(|i| l[i + 8..].trim().to_string())
                    .filter(|s| !s.is_empty());
            }
            Some(l.to_string())
        })
        .collect();
    let joined = kept.join("\n");
    if joined.trim().is_empty() {
        return "（无输出）".into();
    }
    let chars: Vec<char> = joined.chars().collect();
    if chars.len() > MAX {
        format!("…{}", chars[chars.len() - MAX..].iter().collect::<String>())
    } else {
        joined
    }
}

/// 本机 node 主版本号；没有 node / 跑不起来返回 0。
fn node_major() -> u32 {
    let mut c = std::process::Command::new(brains::resolve_bin("node"));
    c.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let Ok(out) = c.output() else { return 0 };
    if !out.status.success() {
        return 0;
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// npm 全局装包：managed=Some((托管 node bin, data_dir)) 时用托管 npm——
/// prefix 指到 <data_dir>/node/npm-global、子进程 PATH 前置托管 bin（npm 脚本自己也要找 node）。
fn npm_install_global(
    pkg: &str,
    proxy: &[(String, String)],
    managed: Option<(&std::path::Path, &std::path::Path)>,
) -> Result<(), String> {
    let mut c = match managed {
        Some((bin, data_dir)) => {
            let mut c = std::process::Command::new(node_boot::managed_npm_exe(bin));
            let sep = if cfg!(windows) { ';' } else { ':' };
            let cur = std::env::var("PATH").unwrap_or_default();
            c.env("PATH", format!("{}{sep}{cur}", bin.to_string_lossy()));
            c.env("npm_config_prefix", node_boot::npm_prefix_dir(data_dir));
            c
        }
        None => std::process::Command::new(brains::resolve_bin("npm")),
    };
    c.args(["install", "-g", pkg]);
    c.envs(proxy.iter().cloned());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().map_err(|e| format!("启动 npm 失败: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(install_err_tail(&out.stdout, &out.stderr))
    }
}

/// 官方原生安装脚本渠道（独立二进制，不需要 Node/npm）。
/// macOS/Linux: curl … install.sh | sh；Windows: PowerShell irm … install.ps1 | iex。
fn install_claude_native(proxy: &[(String, String)]) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut c = {
        let mut c = std::process::Command::new("powershell");
        // 老 Win10 的 PowerShell 5.1 默认不启用 TLS1.2，irm 会直接握手失败——先显式开启。
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command",
            "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor 3072; irm https://claude.ai/install.ps1 | iex"]);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut c = {
        let mut c = std::process::Command::new("/bin/sh");
        c.args(["-c", "curl -fsSL https://claude.ai/install.sh | sh"]);
        c
    };
    c.envs(proxy.iter().cloned());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().map_err(|e| format!("启动安装失败: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        // 取尾不取头：真正的异常在输出末尾（回显/进度行已剥），供 UI 详情区展示。
        Err(install_err_tail(&out.stdout, &out.stderr))
    }
}

/// npm 系 CLI 的统一安装（自举式托管 Node）：
/// ① 系统 Node≥18 → 系统 npm，行为与从前零变化；② 无 → 自举托管 Node（ensure：复用/下载/解压/校验，
/// 进度经 "node-boot" 事件推前端）再用托管 npm 装；③ 自举成功后进程 PATH 前置 + 用户级持久 PATH
///（幂等；写入失败不致命，消息里提示手动）。native_fallback（仅 claude）：以上全失败回落官方脚本。
async fn install_cli_via_node(
    app: AppHandle,
    data_dir: PathBuf,
    pkg: &'static str,
    cli_name: &'static str,
    native_fallback: bool,
) -> Result<String, String> {
    let proxy = system_proxy_env();
    let sys_node = tauri::async_runtime::spawn_blocking(node_major)
        .await
        .map_err(|e| e.to_string())?;
    let mut errs: Vec<String> = Vec::new();
    if sys_node >= 18 {
        // 系统 Node 可用：老路径，零变化。
        let p2 = proxy.clone();
        match tauri::async_runtime::spawn_blocking(move || npm_install_global(pkg, &p2, None))
            .await
            .map_err(|e| e.to_string())?
        {
            Ok(()) => return Ok(format!("{cli_name} 安装完成（npm 渠道），正在重新检测…")),
            Err(e) => errs.push(format!("npm 渠道：{e}")),
        }
    } else {
        // 自举托管 Node：进度推给前端（下载 x% → 解压 → 校验 → 装 CLI）。
        let app2 = app.clone();
        let boot = node_boot::ensure(&data_dir, move |phase, pct| {
            let _ = app2.emit(
                "node-boot",
                serde_json::json!({ "phase": phase, "pct": pct }),
            );
        })
        .await;
        match boot {
            Ok(bin) => {
                let _ = app.emit("node-boot", serde_json::json!({ "phase": "npm", "pct": 0 }));
                let (p2, bin2, dd2) = (proxy.clone(), bin.clone(), data_dir.clone());
                let res = tauri::async_runtime::spawn_blocking(move || {
                    npm_install_global(pkg, &p2, Some((&bin2, &dd2)))
                })
                .await
                .map_err(|e| e.to_string())?;
                match res {
                    Ok(()) => {
                        // 立即生效：进程 PATH 前置（探测/CLI 子进程继承）；再写用户级持久 PATH。
                        node_boot::prepend_process_path(&data_dir);
                        let mut msg =
                            format!("{cli_name} 安装完成（托管 Node 渠道），正在重新检测…");
                        if let Err(e) = node_boot::register_user_path(&[
                            bin,
                            node_boot::npm_global_bin(&data_dir),
                        ]) {
                            msg.push_str(&format!(
                                "（已装好但未能写入系统 PATH，终端里使用需手动加：{e}）"
                            ));
                        }
                        return Ok(msg);
                    }
                    Err(e) => errs.push(format!("托管 npm 渠道：{e}")),
                }
            }
            Err(e) => errs.push(format!("托管 Node 自举：{e}")),
        }
    }
    if native_fallback {
        let p2 = proxy.clone();
        match tauri::async_runtime::spawn_blocking(move || install_claude_native(&p2))
            .await
            .map_err(|e| e.to_string())?
        {
            Ok(()) => return Ok(format!("{cli_name} 安装完成，正在重新检测…")),
            Err(e) => errs.push(format!("官方脚本渠道：{e}")),
        }
    }
    let mut msg = String::from("安装失败：多为网络问题（VPN/代理没覆盖下载域名）");
    if sys_node < 18 {
        msg.push_str(
            "。更稳的替代：先装 Node.js(≥18)，再点一键安装将自动走 npm 通道（npm 遵循系统代理）",
        );
    }
    msg.push_str(&format!("\n——详情（输出末尾）——\n{}", errs.join("\n")));
    Err(msg)
}

/// 一键安装 Claude Code：系统 Node≥18 优先 npm 官方包渠道；无 Node 自举托管 Node 走 npm；
/// 仍失败回落官方原生脚本。全渠道注入系统代理。
#[tauri::command]
async fn install_claude(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    install_cli_via_node(
        app,
        state.data_dir.clone(),
        "@anthropic-ai/claude-code",
        "Claude Code",
        true,
    )
    .await
}

/// 把系统代理补进本进程环境（已有环境变量不覆盖）：Windows 代理软件通常只设注册表
/// 系统代理、不设环境变量，而 claude/codex CLI 只认环境变量——不补的话 GUI 派生的
/// 对话子进程全部直连（用户被迫开 TUN 模式）。之后 spawn 的所有子进程自动继承。
fn apply_system_proxy_to_process() {
    for (k, v) in system_proxy_env() {
        std::env::set_var(k, v);
    }
}

/// 一键安装 Codex：系统 Node≥18 走系统 npm；无 Node 自举托管 Node 再装（无原生脚本兜底）。
#[tauri::command]
async fn install_codex(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let result =
        install_cli_via_node(app, state.data_dir.clone(), "@openai/codex", "Codex", false).await?;
    // 安装完成后立即处理可能遗留的旧 models_cache.json，避免用户第一次对话才发现问题。
    crate::agent::prepare_codex_cache().await?;
    Ok(result)
}

/// 一键安装 Grok CLI（官方脚本 curl | bash，装到 ~/.grok/bin 并自动補 PATH）。
/// Windows 直下 grok 官方原生单文件 exe（URL 口径抄官方 install.sh，见 node_boot 注释）。
/// 跨平台编译（mac 上可编译审读、纯函数有单测），仅 Windows 分支调用。
#[cfg_attr(not(windows), allow(dead_code))]
async fn install_grok_win_binary(
    app: &AppHandle,
    home: &std::path::Path,
) -> Result<String, String> {
    let emit = |phase: &str, pct: u32| {
        let _ = app.emit(
            "node-boot",
            serde_json::json!({ "phase": phase, "pct": pct }),
        );
    };
    // ① 最新 stable 版本号（主源 → GCS 回退）
    let mut version: Option<String> = None;
    let mut last_err = String::new();
    for url in node_boot::grok_version_urls() {
        match node_boot::fetch_text(&url).await {
            Ok(body) => match node_boot::parse_grok_version(&body) {
                Some(v) => {
                    version = Some(v);
                    break;
                }
                None => last_err = format!("{url}: 返回的不是版本号"),
            },
            Err(e) => last_err = format!("{url}: {e}"),
        }
    }
    let Some(version) = version else {
        return Err(format!(
            "安装失败：获取 grok 版本号失败，多为网络问题（代理需覆盖 x.ai）\n——详情——\n{last_err}"
        ));
    };
    // ② 下载单文件 exe（带真实进度）
    let mut data: Option<Vec<u8>> = None;
    for url in node_boot::grok_win_exe_urls(&version, std::env::consts::ARCH) {
        match node_boot::download(&url, &|_, pct| emit("grok-download", pct)).await {
            Ok(b) => {
                data = Some(b);
                break;
            }
            Err(e) => last_err = format!("{url}: {e}"),
        }
    }
    let Some(data) = data else {
        return Err(format!("安装失败：下载 grok 失败，多为网络问题（代理需覆盖 x.ai 与 storage.googleapis.com）\n——详情——\n{last_err}"));
    };
    // ③ 官方安装布局：~/.grok/bin/{grok.exe, agent.exe}
    let bin_dir = node_boot::grok_win_bin_dir(home);
    std::fs::create_dir_all(&bin_dir).map_err(|e| format!("创建 {}: {e}", bin_dir.display()))?;
    for name in ["grok.exe", "agent.exe"] {
        std::fs::write(bin_dir.join(name), &data)
            .map_err(|e| format!("写入 {name} 失败（若在运行请先退出 grok）: {e}"))?;
    }
    // ④ 校验 + PATH：进程内前置立即可被探测/拉起；用户级持久 PATH 幂等追加（失败不致命）。
    emit("grok-verify", 0);
    node_boot::prepend_path_dirs(&[bin_dir.clone()]);
    let mut c = std::process::Command::new(bin_dir.join("grok.exe"));
    c.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let ok = c.output().map(|o| o.status.success()).unwrap_or(false);
    if !ok {
        return Err("安装失败：下载的 grok 无法运行（--version 未通过），可能被安全软件拦截——请重试或手动安装".into());
    }
    let mut msg = format!("Grok CLI {version} 安装完成，正在重新检测…");
    if let Err(e) = node_boot::register_user_path(&[bin_dir]) {
        msg.push_str(&format!(
            "（已装好但未能写入系统 PATH，终端里使用需手动加：{e}）"
        ));
    }
    Ok(msg)
}

/// 一键安装 Grok CLI：mac/linux 走官方脚本（curl | bash）；Windows 直下官方原生 exe——
/// 不再要求 Git Bash（侦查：官方无 npm 包，但 x.ai/cli 有 windows 单文件产物直链）。
#[tauri::command]
async fn install_grok(app: AppHandle) -> Result<String, String> {
    #[cfg(windows)]
    {
        let home = std::env::var("USERPROFILE")
            .map_err(|_| "拿不到用户目录（USERPROFILE）".to_string())?;
        return install_grok_win_binary(&app, std::path::Path::new(&home)).await;
    }
    #[cfg(not(windows))]
    {
        let _ = &app;
        tauri::async_runtime::spawn_blocking(|| {
            let mut c = std::process::Command::new("/bin/bash");
            c.args(["-c", "curl -fsSL https://x.ai/cli/install.sh | bash"]);
            c.envs(system_proxy_env());
            let out = c.output().map_err(|e| format!("启动安装脚本失败: {e}"))?;
            if out.status.success() {
                Ok("Grok CLI 安装完成，正在重新检测…".to_string())
            } else {
                Err(format!(
                    "安装失败：多为网络问题（代理需覆盖 x.ai）\n——详情（输出末尾）——\n{}",
                    install_err_tail(&out.stdout, &out.stderr)
                ))
            }
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

/// 一键安装 wrangler（npm i -g wrangler）。npm 走已修复的 PATH（Windows 上 .cmd 必须
/// 完整路径才能 spawn）；同样注入系统代理。可能耗时半分钟到一两分钟。
/// 安卓手机：adb 在不在、连了哪些手机。
#[tauri::command]
async fn phone_status(state: tauri::State<'_, AppState>) -> Result<phone::PhoneStatus, String> {
    Ok(phone::status(&state.data_dir).await)
}

/// 从 Google 下载官方 platform-tools（adb）到 Pilot 数据目录。
#[tauri::command]
async fn install_adb(state: tauri::State<'_, AppState>) -> Result<String, String> {
    phone::install(&state.data_dir).await
}

/// 邮箱验证码：接了哪个邮箱、最近取过哪些网站的验证码。
#[tauri::command]
fn mail_status(state: tauri::State<'_, AppState>) -> mail::MailStatus {
    mail::MailStatus { google_ready: data_cmds::gmail_client(&state).is_some(), ..mail::status(&state.data_dir) }
}

/// 接邮箱：用应用专用密码真登录一次，成功才保存（密码只进钥匙串）。
#[tauri::command]
async fn mail_connect(state: tauri::State<'_, AppState>, email: String, password: String) -> Result<mail::MailStatus, String> {
    let st = mail::connect(&state.data_dir, &email, &password).await?;
    Ok(mail::MailStatus { google_ready: data_cmds::gmail_client(&state).is_some(), ..st })
}

#[tauri::command]
fn mail_remove(state: tauri::State<'_, AppState>) -> Result<(), String> {
    mail::remove(&state.data_dir)
}

/// 设置里的「填验证码」：用户自己在手机上点了获取验证码、点进了输入框，Pilot 取最新的邮箱验证码填进去（不经过 AI）。
#[tauri::command]
async fn mail_fill_code(state: tauri::State<'_, AppState>, serial: String) -> Result<String, String> {
    mail::fill_code_manual(&state.data_dir, &serial).await
}

/// 输入框的建议回复：这一轮结束后，用同一家的小模型猜用户下一句（见 `suggest`）。拿不到就是空建议。
#[tauri::command]
async fn suggest_reply(state: tauri::State<'_, AppState>, conv_id: String) -> Result<suggest::Suggestion, String> {
    let conv = state.convos.get(&conv_id).ok_or("会话不存在")?;
    Ok(suggest::suggest(&state.data_dir, &conv).await)
}

/// 手机连着电脑时屏幕常亮（开发者选项「不锁定屏幕」）。
#[tauri::command]
async fn phone_stay_awake(state: tauri::State<'_, AppState>, serial: String, on: bool) -> Result<(), String> {
    phone::set_stay_awake(&state.data_dir, &serial, on).await
}

/// 用 Google 登录接 Gmail：打开系统浏览器，等用户点「允许」（最多 5 分钟），成功后把窗口拉回前面。
/// 取消用网站数据那边同一个 `google_oauth_cancel`（一次只等一个浏览器授权）。
#[tauri::command]
async fn mail_google_connect(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<mail::MailStatus, String> {
    use tauri_plugin_opener::OpenerExt;
    let client = data_cmds::gmail_client(&state).ok_or("还没有配置 Google OAuth 客户端")?;
    let st = mail::google_connect(&state.data_dir, client, |url| app.opener().open_url(url, None::<&str>).map_err(|e| format!("打开浏览器失败：{e}"))).await?;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_focus();
    }
    Ok(mail::MailStatus { google_ready: true, ..st })
}

#[tauri::command]
async fn install_wrangler() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut c = std::process::Command::new(brains::resolve_bin("npm"));
        c.args(["install", "-g", "wrangler"]);
        c.envs(system_proxy_env());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let out = c
            .output()
            .map_err(|e| format!("运行 npm 失败（确认已装 Node/npm）: {e}"))?;
        if out.status.success() {
            Ok("wrangler 安装完成".to_string())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            let last: String = err
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("未知错误")
                .chars()
                .take(200)
                .collect();
            Err(format!("安装失败：{last}"))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn list_conversations(state: tauri::State<'_, AppState>) -> Vec<Conversation> {
    state.convos.list()
}

#[tauri::command]
fn get_conversation(state: tauri::State<'_, AppState>, id: String) -> Option<Conversation> {
    state.convos.get(&id)
}

/// 进行中的对话连同写到一半的回复（每 3 秒存一次）：页面重载、开了第二个窗口，丢了这一轮的实时通道时用它接着显示。
#[tauri::command]
fn get_conversation_live(state: tauri::State<'_, AppState>, id: String) -> Option<Conversation> {
    state.convos.get_live(&id)
}

#[tauri::command]
fn delete_conversation(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.convos.remove(&id)
}

#[tauri::command]
fn cancel_turn(state: tauri::State<'_, AppState>, conv_id: String) -> bool {
    state.runs.cancel(&conv_id)
}

/// 会话进行中切换模型（同厂商档位）：仅改存储里的 model，下一轮 send_message 读到即用；
/// 厂商/站点不可改（session/thread 与厂商绑定，站点决定 api/key/cwd）。
#[tauri::command]
fn set_conversation_model(
    state: tauri::State<'_, AppState>,
    conv_id: String,
    model: String,
) -> Result<Option<Conversation>, String> {
    state.convos.mutate_checked(&conv_id, now_secs(), |c| {
        if c.brain.starts_with("api:") && c.status == "running" {
            return Err("上一轮还在进行中，请稍候再切换 API 模型".into());
        }
        validate_executor(&state.data_dir, &c.brain, &model)?;
        let effort = conversation_effort_after_switch(&state.data_dir, c, &c.brain, &model)?;
        let changed = c.model != model;
        c.model = model;
        c.effort = effort;
        // ★ claude 的 `--resume` 无视 `--model`（2.1.96 实测：resume 换档后自报仍是旧模型）——
        // 「同厂商换档下一轮生效」对 claude 是空头支票。换模型就清 session_ref，
        // 下一条消息走 StartedFresh 重建路（新 session + 历史摘要），模型才真正切换。
        // codex/grok 的模型是逐轮配置下发，不受此坑影响，session 照留。
        if changed && (c.brain == "claude" || c.brain.starts_with("api:")) {
            c.session_ref = String::new();
        }
        Ok(())
    })
}

fn api_profile_for_brain(data_dir: &std::path::Path, brain: &str) -> Result<Option<api_profiles::ApiProfile>, String> {
    let Some(id) = brain.strip_prefix("api:") else {
        return Ok(None);
    };
    let (_lock, profile) = api_profiles::locked_profile(data_dir, id)?;
    Ok(Some(profile))
}

fn conversation_effort_after_switch(
    data_dir: &std::path::Path,
    c: &Conversation,
    brain: &str,
    model: &str,
) -> Result<String, String> {
    let new_profile = api_profile_for_brain(data_dir, brain)?;
    let new_protocol = new_profile.as_ref().map(|p| p.protocol.as_str());
    let effective_model = new_profile.as_ref()
        .filter(|_| model.trim().is_empty())
        .map(|p| p.default_model.as_str())
        .unwrap_or(model);
    let old_profile = if c.brain == brain {
        None
    } else {
        api_profile_for_brain(data_dir, &c.brain).ok().flatten()
    };
    let old_protocol = if c.brain == brain {
        new_protocol
    } else {
        old_profile.as_ref().map(|p| p.protocol.as_str())
    };
    Ok(api_reasoning::after_model_switch(
        &c.brain, brain, old_protocol, new_protocol, effective_model, &c.effort,
    ))
}

/// 跨厂商切换的会话字段改写：brain+model 一起换。换厂商时清空 session_ref——
/// 旧厂商的 session 另一家恢复不了，留着只会让失败路径拿旧 ref 去 resume 新厂商；
/// 调用方随后必须走 rebuild_session 以历史摘要重建续跑（begin_rebuild 本就不要求已有 session）。
/// CLI 间 effort 保留；API 思考等级在调用处按协议/模型原子归一化。
/// codex 不支持逐命令确认，ask/auto 档按启动器同规则落到 full
///（claude/grok 都有逐命令批准，互切时 perm 原样保留）。
///
/// ★ `is_ssh` 例外：远程连接下**绝不能**把 ask/auto 改写成 full。那里 codex 虽然仍没有本地逐命令闸，
/// 但走 AI 桥的远程命令**照样逐条弹卡**（闸在 bridge.rs，与厂商无关）——那是 codex 仅剩的一道闸。
/// 把它也拿掉，用户就会「明明选了自动，却在无人确认地动真机」。这是评审里的 critical，别改回去。
fn apply_brain_switch(c: &mut Conversation, brain: &str, model: &str, is_ssh: bool) {
    if c.brain != brain {
        c.session_ref = String::new();
        if brain == "codex" && !is_ssh && (c.perm_mode == "ask" || c.perm_mode == "auto") {
            c.perm_mode = "full".into();
        }
    }
    c.brain = brain.into();
    c.model = model.into();
}

/// 会话内跨厂商换模型（含同厂商，前端只在跨厂商时调它）。运行中拒绝：
/// 本轮已带旧厂商启动，中途换 brain 会让收尾逻辑把新厂商会话和旧 session 缝在一起。
#[tauri::command]
fn set_conversation_brain_model(
    state: tauri::State<'_, AppState>,
    conv_id: String,
    brain: String,
    model: String,
) -> Result<Option<Conversation>, String> {
    validate_executor(&state.data_dir, &brain, &model)?;
    state.convos.mutate_checked(&conv_id, now_secs(), |c| {
        if c.status == "running" {
            return Err("上一轮还在进行中，请稍候".into());
        }
        // ssh 对话切厂商时不许把 ask/auto 改写成 full（见 apply_brain_switch）。
        let is_ssh = c.task_type == "workspace"
                || state
                    .conns
                    .get(&c.conn_id)
                    .map(|x| x.kind == "ssh")
                    .unwrap_or(false);
        let effort = conversation_effort_after_switch(&state.data_dir, c, &brain, &model)?;
        apply_brain_switch(c, &brain, &model, is_ssh);
        c.effort = effort;
        Ok(())
    })
}

/// 会话进行中切换权限档位（plan/ask/auto/full）：仅改存储，下一轮 send_message 读到即用。
/// 注意 claude 把权限档位钉在会话创建时，改基座模式要新对话才全生效；但钩子每轮重生成，
/// 故「自动/询问」的拦截逻辑仍随时可调（详见 permit.rs）。
#[tauri::command]
fn set_conversation_effort(
    state: tauri::State<'_, AppState>,
    conv_id: String,
    effort: String,
) -> Result<Option<convo::Conversation>, String> {
    state.convos.mutate_checked(&conv_id, now_secs(), |c| {
        if let Some(profile) = api_profile_for_brain(&state.data_dir, &c.brain)? {
            let model = if c.model.trim().is_empty() { &profile.default_model } else { &c.model };
            api_reasoning::validate(&profile.protocol, model, &effort)?;
        }
        c.effort = effort;
        Ok(())
    })
}

/// Claude CLI 的可升级情况。**只读**：查 npm registry 的已发布版本，不碰本机安装。
#[derive(serde::Serialize)]
struct CliUpdateInfo {
    installed: String,
    latest: String,
    has_update: bool,
    /// 会被真正升级的那一个（= PATH 里第一个）。
    target: String,
    /// PATH 上全部同名安装。>1 时 UI 必须警示：只有第一个生效。
    all: Vec<String>,
    /// 升到 latest 之后能不能用 headless fast mode（当前是否已够也看 brains.claude_fast_ok）。
    fast_after_upgrade: bool,
}

/// 查有没有新版。**故意不用 `claude update`**：它没有「只检查」模式
///（`--help` 原文 "Check for updates and install if available"），调用即安装 ——
/// 不能为了显示一个版本号就擅自改用户的环境。npm registry 是只读且够权威的来源
///（实测与 CLI changelog 的最新条目一致）。
#[tauri::command]
async fn check_claude_update() -> Result<CliUpdateInfo, String> {
    let all = brains::which_all("claude");
    let target = all.first().cloned().unwrap_or_default();
    if target.is_empty() {
        return Err("PATH 上找不到 claude".into());
    }
    let installed_raw = tokio::process::Command::new(&target)
        .arg("--version")
        .output()
        .await
        .map_err(|e| format!("读取本机版本失败: {e}"))?;
    let installed = String::from_utf8_lossy(&installed_raw.stdout).trim().to_string();

    let out = tokio::process::Command::new(brains::resolve_bin("npm"))
        .args(["view", "@anthropic-ai/claude-code", "version"])
        .output()
        .await
        .map_err(|e| format!("查询最新版失败（需要 npm）: {e}"))?;
    let latest = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if latest.is_empty() {
        return Err("没查到已发布版本（网络或 npm 不可用）".into());
    }
    let cur = agent::parse_version_triplet(&installed);
    let new = agent::parse_version_triplet(&latest);
    // ★ 版本号必须按数值三元组比，不能比字符串："2.1.96" > "2.1.220" 在字符串序下成立，
    //   那会把「有新版」判成「已最新」，而且不报错。
    let has_update = matches!((cur, new), (Some(c), Some(n)) if n > c);
    Ok(CliUpdateInfo {
        installed,
        latest,
        has_update,
        target,
        all,
        fast_after_upgrade: new.is_some_and(|n| n >= brains::FAST_MIN_CLI),
    })
}

/// 执行升级：对 **PATH 里第一个** claude 跑它自带的 `update`。
/// 前端必须先弹确认（会改本机环境），这里只负责执行并把原始输出回传 —— 包括多安装警告那类
/// 信息，别替用户吞掉。
#[tauri::command]
async fn upgrade_claude_cli() -> Result<String, String> {
    let target = brains::which_all("claude")
        .first()
        .cloned()
        .ok_or_else(|| "PATH 上找不到 claude".to_string())?;
    let out = tokio::process::Command::new(&target)
        .arg("update")
        .output()
        .await
        .map_err(|e| format!("执行升级失败: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !err.is_empty() {
        text = if text.is_empty() { err } else { format!("{text}\n{err}") };
    }
    if !out.status.success() {
        return Err(if text.is_empty() { "升级失败（无输出）".into() } else { text });
    }
    Ok(text)
}

/// 运行中改 fast：和 effort 一样每轮下发，下一轮生效。
#[tauri::command]
fn set_conversation_fast(
    state: tauri::State<'_, AppState>,
    conv_id: String,
    fast: bool,
) -> Result<Option<convo::Conversation>, String> {
    state.convos.mutate(&conv_id, now_secs(), move |c| c.fast = fast)
}

#[tauri::command]
fn set_conversation_perm_mode(
    state: tauri::State<'_, AppState>,
    conv_id: String,
    perm_mode: String,
) -> Result<Option<Conversation>, String> {
    let to_full = perm_mode == "full";
    let updated = state
        .convos
        .mutate(&conv_id, now_secs(), move |c| c.perm_mode = perm_mode)?;
    // ★ 切到「全自动」对**正在跑的这一轮**实时生效：已弹出/将弹出的批准卡全部自动放行
    //（钩子在轮询应答文件，写 allow 即放行；后续新请求由 list_pending_permits 兜）。
    // 收紧方向（full→询问 等）只能下一轮生效——full 那轮根本没装钩子，无从拦起。
    if to_full {
        permit::auto_allow_for(&state.data_dir.join("permit").join("pending"), &conv_id);
    }
    Ok(updated)
}

/// 列出所有等待用户批准的工具调用（钩子写在 <data_dir>/permit/pending）。UI 轮询它渲染批准卡。
/// 会话已切到「全自动」的请求不出卡：本轮是带旧档启动的，钩子还在陆续写新请求——
/// 轮询到这里就地放行，让「运行中切全自动」真正实时生效。
#[tauri::command]
fn list_pending_permits(state: tauri::State<'_, AppState>) -> Vec<permit::PendingPermit> {
    let dir = state.data_dir.join("permit").join("pending");
    let all = permit::list_pending(&dir);
    if all.is_empty() {
        return all;
    }
    let full: std::collections::HashSet<String> = state
        .convos
        .list()
        .into_iter()
        .filter(|c| c.perm_mode == "full")
        .map(|c| c.id)
        .collect();
    // 运营任务的无人值守会话：一律拒绝，不出卡（推送、部署等由 Pilot 自己按档位执行）。
    let (unattended, all): (Vec<_>, Vec<_>) = all.into_iter().partition(|p| site_runner::is_unattended(&p.conv));
    for p in &unattended {
        let _ = permit::respond(&dir, &p.id, false);
    }
    let (auto, show): (Vec<_>, Vec<_>) = all.into_iter().partition(|p| full.contains(&p.conv));
    for p in &auto {
        let _ = permit::respond(&dir, &p.id, true);
    }
    show
}

/// 应答一条待批请求：allow=放行 / 否则拒绝。钩子轮询到响应文件后据此放行或拦截。
#[tauri::command]
fn respond_permit(
    state: tauri::State<'_, AppState>,
    id: String,
    allow: bool,
) -> Result<(), String> {
    permit::respond(&state.data_dir.join("permit").join("pending"), &id, allow)
}

// ---- 定时任务 ----

fn user_msg(text: String, now: u64) -> Message {
    Message {
        role: "user".into(),
        text,
        tools: vec![],
        ts: now,
        hidden: false,
        error: false,
        limit_reset: None,
        partial: false,
    }
}
fn assistant_msg(res: &agent::TurnResult, now: u64, brain: &str) -> Message {
    Message {
        role: "assistant".into(),
        text: if brain.starts_with("api:") && !res.ok && !res.text.trim().is_empty() {
            format!("执行未完成：{}\n\n以下为中断前的部分输出：\n{}", res.error, res.text)
        } else if res.text.trim().is_empty() && !res.ok {
            res.error.clone()
        } else {
            res.text.clone()
        },
        tools: res.tools.clone(),
        ts: now,
        hidden: false,
        // API 的部分输出不代表成功，失败必须保留错误状态。CLI 沿用原有规则：
        // 只有「失败且没产出内容」才标红（展示错误信息）；有正常回答就不标红——
        // CLI 有时退出码非 0 但回答是好的（如跑了 open 打开浏览器），不该把整段染红。
        error: !res.ok && (res.text.trim().is_empty() || brain.starts_with("api:")),
        limit_reset: res.limit_reset,
        partial: false,
    }
}

fn apply_usage(c: &mut Conversation, res: &agent::TurnResult, data_dir: &std::path::Path) {
    if let Some(u) = &res.usage {
        let ctx = u.input + u.cache_read + u.cache_create;
        c.ctx_tokens = if c.brain == "codex" || c.brain.starts_with("api:") { 0 } else { ctx };
        c.total_tokens += ctx + u.output; // 累计（每轮全量计入，反映实际处理量）
        c.billed_tokens += billed_tokens(u);
        usage::append(
            data_dir,
            &usage::UsageEntry {
                ts: now_secs() as i64,
                brain: c.brain.clone(),
                model: c.model.clone(),
                input: u.input,
                output: u.output,
                cache_read: u.cache_read,
                cache_create: u.cache_create,
                rough: false,
            },
        );
        // 运营作业开的会话：另记到站点自己的用量里（按月看每个站点的 AI 花费）
        if c.origin == "ops" {
            if let Some(site_id) = c.conn_id.strip_prefix("site:") {
                usage::append_site(
                    &data_dir.join("sites").join(site_id),
                    &usage::UsageEntry { ts: now_secs() as i64, brain: c.brain.clone(), model: c.model.clone(), input: u.input, output: u.output, cache_read: u.cache_read, cache_create: u.cache_create, rough: false },
                );
            }
        }
    }
}

/// 本地用量参考统计：两个起点（前端按本地时区算好，如近 5 小时 / 今日零点）各汇总一份。
#[tauri::command]
fn usage_stats(state: tauri::State<'_, AppState>, since_a: i64, since_b: i64) -> usage::UsageStats {
    usage::stats(&state.data_dir, since_a, since_b)
}

/// 会话系统提示（首轮与重建共用同一份规矩）：自由对话 / SSH 运维 / Cloudflare 建站。
/// 截图能力只给 Cloudflare：SSH 与自由对话用不上，只是白占提示词。
fn conversation_system_prompt(
    conn: &conns::Connection,
    is_workspace: bool,
    workspace_dir: &str,
    site_slug: &str,
    work_dir: &str,
    brain: &str,
    data_dir: &std::path::Path,
) -> String {
    if is_workspace {
        return format!("{}\n\n{}", agent::workspace_system_prompt(!workspace_dir.trim().is_empty()), phone::phone_prompt(&tools::phone_js_path(data_dir).to_string_lossy()));
    }
    if conn.kind == "github" {
        return agent::github_system_prompt(site_slug, &conn.login);
    }
    if conn.kind == "ssh" {
        return agent::ssh_system_prompt(
            &conn.ssh_user,
            &conn.ssh_host,
            conn.ssh_port,
            &tools::ssh_js_path(data_dir).to_string_lossy(),
        );
    }
    // 项目目录非空（引用了模板或上一轮已建过）时，让模型先读现有文件再改。
    let sys = agent::cf_system_prompt(
        &cf_project_slug(site_slug),
        &conn.account_id,
        dir_has_content(std::path::Path::new(work_dir)),
    );
    let shot = data_dir.join("tools").join("shot.js");
    let sys = format!("{sys}\n\n{}", agent::shot_prompt(&shot.to_string_lossy()));
    design_fallback(sys, true, brain, data_dir)
}

/// 开新会话的共享实现（命令与定时任务调度器都用它）。stores 传入克隆，便于后台任务持有。
/// conv_id 由调用方生成；session_ref 首轮先留空，跑完才回填（崩溃留空→续对话给「已失效」）。
#[allow(clippy::too_many_arguments)]
async fn create_conversation(
    conns: conns::ConnStore,
    sites: sites::SiteStore,
    convos: convo::ConvStore,
    runs: agent::RunRegistry,
    conv_id: String,
    conn_id: String,
    site_slug: String,
    site_name: String,
    task_type: String,
    brain: String,
    model: String,
    perm_mode: String,
    effort: String,
    // fast 模式（Opus 5 / Opus 4.8）。tauri 命令参数不吃 serde 属性，前端必须显式传。
    fast: bool,
    workspace_dir: String,
    message: String,
    on_event: Channel<agent::TurnEvent>,
    data_dir: PathBuf,
    ssh: ssh::SshSessions,
    api_execution_policy: api_execution::ApiExecutionPolicy,
    origin: String,
) -> Result<Conversation, String> {
    validate_executor(&data_dir, &brain, &model)?;
    let now = now_secs();
    // API 提前保存引用：即使首轮工具执行时进程退出，也不能把下一轮当成全新任务重放。
    let session_seed = fresh_session_seed(&brain);
    let is_workspace = task_type == "workspace";
    let is_site = task_type == "site";
    // 本地对话只属于虚拟的本地工作区，不借用任何真实连接。
    if is_workspace && conn_id != WORKSPACE_CONNECTION_ID {
        return Err("本地对话必须在本地工作区中创建".into());
    }
    let (conn, work_dir, sys, stored_conn_id, stored_conn_name) = if is_site {
        // 站点会话：侧栏归属 site:<id>，运行时借用站点的代码连接（见 site_cmds::site_runtime）。
        let (conn, work_dir, _, site) = site_cmds::site_runtime(&sites, &conns, site_slug.trim())?;
        if conn_id != site_cmds::site_conn_id(&site.id) {
            return Err("站点会话归属不匹配".into());
        }
        let work_dir = site_cmds::site_work_override(&sites, &site.id, &workspace_dir).unwrap_or(work_dir);
        let sys = site_prompt(&site, &conn, &data_dir, &sites);
        (conn, work_dir, sys, conn_id.clone(), site.name.clone())
    } else {
        let work_dir = if is_workspace {
            resolve_workspace_dir(&data_dir, &conv_id, &workspace_dir)?
        } else {
            let conn = conns.get(&conn_id)?;
            resolve_work_dir(&conn, site_slug.trim())?
        };
        let conn = if is_workspace {
            workspace_connection(&work_dir)
        } else {
            conns.get(&conn_id)?
        };
        let is_cf = conn.kind == "cloudflare";
        let is_ssh = conn.kind == "ssh";
        if !is_workspace && !is_ssh && !is_cf && conn.kind != "github" {
            return Err(format!("不支持的连接类型：{}", conn.kind));
        }
        let sys = conversation_system_prompt(
            &conn,
            is_workspace,
            &workspace_dir,
            site_slug.trim(),
            &work_dir,
            &brain,
            &data_dir,
        );
        let (stored_conn_id, stored_conn_name) = if is_workspace {
            (WORKSPACE_CONNECTION_ID.to_string(), "本地对话".to_string())
        } else {
            (conn.id.clone(), conn.name.clone())
        };
        (conn, work_dir, sys, stored_conn_id, stored_conn_name)
    };
    let task_type_for_turn = task_type.clone();
    let conv = Conversation {
        id: conv_id.clone(),
        conn_id: stored_conn_id,
        conn_name: stored_conn_name,
        site_slug: site_slug.trim().to_string(),
        site_name,
        workspace_dir: if is_workspace || is_site {
            workspace_dir.trim().to_string()
        } else {
            String::new()
        },
        task_type,
        brain: brain.clone(),
        model: model.clone(),
        perm_mode: perm_mode.clone(),
        effort: effort.clone(),
        fast,
        session_ref: if brain.starts_with("api:") { session_seed.clone() } else { String::new() },
        title: title_from(&message),
        messages: vec![user_msg(message.trim().to_string(), now)],
        status: "running".into(),
        created_at: now,
        updated_at: now,
        ctx_tokens: 0,
        total_tokens: 0,
        billed_tokens: 0,
        origin,
    };
    convos.upsert(conv)?;

    let res = agent::run_turn(
        runs,
        conn,
        work_dir,
        brain,
        model,
        perm_mode,
        effort,
        fast,
        data_dir.clone(),
        ssh,
        session_seed,
        true,
        Some(sys),
        turn_message(&task_type_for_turn, message.trim().to_string()),
        conv_id.clone(),
        turn_checkpoint::wrap(convos.clone(), conv_id.clone(), on_event),
        api_execution_policy,
    )
    .await;

    let now2 = now_secs();
    let updated = convos.mutate(&conv_id, now2, |c| {
        // Claude 首轮会预生成 --session-id；若 CLI 连进程都没启动，run_turn 的失败结果
        // 仍会带回这个种子。失败时绝不能落库，否则“重试”会拿幽灵 id 去 --resume，
        // 随后只能得到 `No conversation found with session ID`。
        if session_ref_is_persistable(&c.brain, res.ok, &res.session_ref) {
            c.session_ref = res.session_ref.clone();
        }
        c.status = "idle".into();
        c.messages.retain(|m| !m.partial); // 边跑边存的那份草稿换成正式回复
        c.messages.push(assistant_msg(&res, now2, &c.brain));
        apply_usage(c, &res, &data_dir);
    })?;
    updated.ok_or_else(|| "会话丢失".into())
}

/// 发给执行器的本轮消息：站点会话改的是面向访客的内容，和 CF 建站一样附带公开文案规范
/// （每轮注入，覆盖已建立的会话；落库的仍是用户原文）。
fn turn_message(task_type: &str, message: String) -> String {
    if task_type == "site" {
        editorial::for_turn(message, "cloudflare")
    } else {
        message
    }
}

/// 站点会话的系统提示（首轮与重建共用）。
fn site_prompt(
    site: &sites::Site,
    conn: &conns::Connection,
    data_dir: &std::path::Path,
    sites: &sites::SiteStore,
) -> String {
    let tool = site_data::tool_prompt(data_dir, &sites.data_dir(&site.id));
    let shot = data_dir.join("tools").join("shot.js");
    let phone = tools::phone_js_path(data_dir);
    format!("{}\n\n{}\n\n{}", site_cmds::site_system_prompt(site, &conn.login, &tool), agent::site_shot_prompt(&shot.to_string_lossy()), phone::phone_prompt(&phone.to_string_lossy()))
}

fn fresh_session_seed(brain: &str) -> String {
    if brain.starts_with("api:") {
        format!("api:{}", uuid::Uuid::new_v4())
    } else if brain == "claude" {
        uuid::Uuid::new_v4().to_string()
    } else {
        String::new()
    }
}

fn session_ref_is_persistable(brain: &str, turn_ok: bool, session_ref: &str) -> bool {
    if brain.starts_with("api:") {
        // API references are reserved before running, and transcripts precede tool execution.
        // Even a missing transcript must fail closed, never silently start over after a crash.
        return session_ref.strip_prefix("api:")
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
    }
    turn_ok && !session_ref.is_empty()
}

#[cfg(test)]
mod conversation_session_tests {
    use super::{fresh_session_seed, session_ref_is_persistable};

    #[test]
    fn failed_first_turn_does_not_persist_preallocated_claude_session() {
        assert!(!session_ref_is_persistable(
            "claude",
            false,
            "75739ec2-daa6-44c0-930c-30309ca88e45"
        ));
    }

    #[test]
    fn successful_turn_requires_a_non_empty_session_reference() {
        assert!(session_ref_is_persistable("claude", true, "session-created-by-cli"));
        assert!(!session_ref_is_persistable("claude", true, ""));
    }

    #[test]
    fn api_failed_turn_keeps_persisted_history_but_not_a_cli_seed() {
        let saved = "api:75739ec2-daa6-44c0-930c-30309ca88e45";
        assert!(session_ref_is_persistable("api:profile", false, saved));
        assert!(session_ref_is_persistable("api:profile", true, saved));
        for invalid in ["", "api:invalid", "75739ec2-daa6-44c0-930c-30309ca88e45"] {
            assert!(!session_ref_is_persistable("api:profile", false, invalid));
        }
        assert!(!session_ref_is_persistable("claude", false, saved));
    }

    #[test]
    fn api_reserves_distinct_valid_refs_before_execution() {
        let first = fresh_session_seed("api:profile");
        assert!(session_ref_is_persistable("api:profile", false, &first));
        assert_ne!(first, fresh_session_seed("api:profile"));
        assert!(uuid::Uuid::parse_str(&fresh_session_seed("claude")).is_ok());
        assert!(fresh_session_seed("codex").is_empty());
        assert!(fresh_session_seed("grok").is_empty());
    }
}

#[cfg(test)]
mod api_outcome_integration_tests {
    use super::*;

    fn partial_result() -> agent::TurnResult {
        agent::TurnResult {
            ok: false, text: "部分输出".repeat(200), tools: vec![],
            error: "model unavailable: stream interrupted".into(),
            session_ref: fresh_session_seed("api:profile"),
            usage: None, limit_reset: None,
        }
    }

    #[test]
    fn partial_api_error_stays_failed() {
        let result = partial_result();
        let message = assistant_msg(&result, 10, "api:profile");
        assert!(message.error);
        assert!(message.text.starts_with("执行未完成：model unavailable"));
        assert!(!assistant_msg(&result, 10, "claude").error, "legacy CLI behavior is unchanged");
    }
}

/// 是否已经有可操作目标：自由对话与 SSH 不需要项目；Cloudflare 必须先有项目 slug。
fn conversation_target_ready(conn_kind: &str, task_type: &str, site_slug: &str) -> bool {
    if conn_kind == "github" {
        return github::parse_repo(site_slug).is_ok();
    }
    task_type == "workspace" || conn_kind == "ssh" || !site_slug.trim().is_empty()
}

#[cfg(test)]
mod conversation_target_tests {
    use super::conversation_target_ready;

    #[test]
    fn cloudflare_needs_a_project_but_ssh_and_workspace_do_not() {
        assert!(!conversation_target_ready("cloudflare", "sitebuild", ""));
        assert!(conversation_target_ready("cloudflare", "sitebuild", "blog"));
        assert!(conversation_target_ready("ssh", "remote", ""));
        assert!(conversation_target_ready("workspace", "workspace", ""));
        assert!(conversation_target_ready("github", "repo", "octo/hello"));
        assert!(!conversation_target_ready("github", "repo", ""));
        assert!(!conversation_target_ready("github", "repo", "../etc"));
    }
}

/// 开新会话（前端命令）：conv_id 前端生成，从首轮就能流式渲染 + 停止。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn start_conversation(
    state: tauri::State<'_, AppState>,
    conv_id: String,
    conn_id: String,
    site_slug: String,
    site_name: String,
    task_type: String,
    brain: String,
    model: String,
    perm_mode: String,
    effort: String,
    fast: bool,
    workspace_dir: String,
    message: String,
    on_event: Channel<agent::TurnEvent>,
    timeout_seconds: Option<u64>,
) -> Result<Conversation, String> {
    if conv_id.trim().is_empty() {
        return Err("会话 id 缺失".into());
    }
    // 自由对话与 SSH 不要求项目；Cloudflare 会话必须有明确项目。
    let conn_kind = if task_type == "workspace" {
        "workspace".to_string()
    } else if task_type == "site" {
        state.sites.get(&site_slug)?;
        "site".to_string()
    } else {
        state.conns.get(&conn_id)?.kind
    };
    if !conversation_target_ready(&conn_kind, &task_type, &site_slug) {
        return Err(if conn_kind == "github" { "请选择一个仓库（owner/repo）" } else { "项目不能为空" }.into());
    }
    if message.trim().is_empty() {
        return Err("请先说点什么".into());
    }
    // Only callers requesting a bounded job get a deadline; normal chats and
    // scheduled jobs retain their existing lifecycle. Do not drop run_turn on
    // timeout: let its cancellation branch terminate the process tree and save history.
    let deadline = timeout_seconds.map(|seconds| {
        turn_deadline::TurnDeadline::start(
            state.runs.clone(),
            conv_id.clone(),
            std::time::Duration::from_secs(seconds.clamp(60, 900)),
        )
    });
    let result = create_conversation(
        state.conns.clone(),
        state.sites.clone(),
        state.convos.clone(),
        state.runs.clone(),
        conv_id,
        conn_id,
        site_slug,
        site_name,
        task_type,
        brain,
        model,
        perm_mode,
        effort,
        fast,
        workspace_dir,
        message,
        on_event,
        state.data_dir.clone(),
        state.ssh.clone(),
        Default::default(),
        String::new(),
    )
    .await;
    if deadline.as_ref().is_some_and(|guard| guard.expired()) {
        return Err("本轮生成已超过时限并停止。过程记录保留在对话中，请检查模型连接或待确认操作后再重试。".into());
    }
    result
}

#[tauri::command]
async fn send_message(
    state: tauri::State<'_, AppState>,
    conv_id: String,
    message: String,
    on_event: Channel<agent::TurnEvent>,
) -> Result<Conversation, String> {
    if message.trim().is_empty() {
        return Err("消息不能为空".into());
    }
    let conv = state.convos.get(&conv_id).ok_or("会话不存在")?;
    let now = now_secs();
    // 原子开始：技能包门闩内再进入会话锁，检查 running/session、
    // 追加用户消息并置 running，杜绝维护与新回合互相穿过。
    let turn_start = state
        .convos
        .begin_turn(&conv_id, now, user_msg(message.trim().to_string(), now));
    match turn_start {
        convo::TurnStart::NotFound => return Err("会话不存在".into()),
        convo::TurnStart::Busy => return Err("上一轮还在进行中，请稍候".into()),
        convo::TurnStart::Failed(e) => return Err(format!("保存会话失败，这条消息没有发出：{e}")),
        convo::TurnStart::StartedFresh => {
            // 老对话的底层会话不存在（当年首轮 401 就崩 / 换厂商清空）——不再死路一条：
            // 按重建口径新起 session（同款系统提示 + 历史摘要）原地续跑这条新消息。
            return run_fresh_turn(&state, conv_id, message.trim().to_string(), true, on_event).await;
        }
        convo::TurnStart::Started => {}
    }
    let (conn, work_dir) = match conversation_runtime(&state.conns, &state.sites, &state.data_dir, &conv) {
        Ok(v) => v,
        Err(e) => {
            abort_turn(&state, &conv_id, true);
            return Err(e);
        }
    };

    let res = agent::run_turn(
        state.runs.clone(),
        conn,
        work_dir,
        conv.brain.clone(),
        conv.model.clone(),
        conv.perm_mode.clone(),
        conv.effort.clone(),
        conv.fast,
        state.data_dir.clone(),
        state.ssh.clone(),
        conv.session_ref.clone(),
        false,
        None,
        turn_message(&conv.task_type, message.trim().to_string()),
        conv_id.clone(),
        turn_checkpoint::wrap(state.convos.clone(), conv_id.clone(), on_event),
        Default::default(),
    )
    .await;

    let now2 = now_secs();
    let updated = state.convos.mutate(&conv_id, now2, |c| {
        c.status = "idle".into();
        c.messages.retain(|m| !m.partial); // 边跑边存的那份草稿换成正式回复
        c.messages.push(assistant_msg(&res, now2, &c.brain));
        apply_usage(c, &res, &state.data_dir);
    })?;
    updated.ok_or_else(|| "会话丢失".into())
}

/// 重试上一轮：去掉失败/部分的助手消息，用最后一条用户消息 resume 再跑一轮（不新增用户消息）。
#[tauri::command]
async fn retry_turn(
    state: tauri::State<'_, AppState>,
    conv_id: String,
    on_event: Channel<agent::TurnEvent>,
) -> Result<Conversation, String> {
    let now = now_secs();
    let message = state.convos.begin_retry(&conv_id, now)?;
    let conv = state.convos.get(&conv_id).ok_or("会话不存在")?;
    let (conn, work_dir) = match conversation_runtime(&state.conns, &state.sites, &state.data_dir, &conv) {
        Ok(v) => v,
        Err(e) => {
            abort_turn(&state, &conv_id, false);
            return Err(e);
        }
    };

    let res = agent::run_turn(
        state.runs.clone(),
        conn,
        work_dir,
        conv.brain.clone(),
        conv.model.clone(),
        conv.perm_mode.clone(),
        conv.effort.clone(),
        conv.fast,
        state.data_dir.clone(),
        state.ssh.clone(),
        conv.session_ref.clone(),
        false,
        None,
        turn_message(&conv.task_type, message),
        conv_id.clone(),
        turn_checkpoint::wrap(state.convos.clone(), conv_id.clone(), on_event),
        Default::default(),
    )
    .await;

    let now2 = now_secs();
    let updated = state.convos.mutate(&conv_id, now2, |c| {
        c.status = "idle".into();
        c.messages.retain(|m| !m.partial); // 边跑边存的那份草稿换成正式回复
        c.messages.push(assistant_msg(&res, now2, &c.brain));
        apply_usage(c, &res, &state.data_dir);
    })?;
    updated.ok_or_else(|| "会话丢失".into())
}

/// 重建会话续跑：底层 CLI 会话状态损坏、重试无效时，抛弃旧 session，
/// 用**全新会话**（原系统提示 + 历史记录摘要）重跑最后一条用户请求。
/// 对用户而言＝这条对话原地继续；成功后把新 session id 接回本会话。
#[tauri::command]
async fn rebuild_session(
    state: tauri::State<'_, AppState>,
    conv_id: String,
    on_event: Channel<agent::TurnEvent>,
) -> Result<Conversation, String> {
    let now = now_secs();
    let last_user = state.convos.begin_rebuild(&conv_id, now)?;
    run_fresh_turn(&state, conv_id, last_user, false, on_event).await
}

/// 这一轮还没真正跑起来就失败了（连接被删、工作文件夹不可用、站点不存在…）：
/// 把 begin_* 已经写进磁盘的 running 收回，不然会话会一直显示「上一轮还在进行中」，
/// 只能重启 Pilot。`pop_user`＝这一轮刚追加的用户消息也一并撤掉（重试 / 重建没有追加，不撤）。
fn abort_turn(state: &AppState, conv_id: &str, pop_user: bool) {
    let _ = state.convos.mutate(conv_id, now_secs(), |c| {
        c.status = "idle".into();
        c.messages.retain(|m| !m.partial);
        if pop_user && c.messages.last().is_some_and(|m| m.role == "user") {
            c.messages.pop();
        }
    });
}

/// 新会话首轮续跑——rebuild_session 与 send_message「复活老对话」的共用核心：
/// 同款系统提示 + 历史摘要（recap 自动跳过将要重跑的最后一条用户消息）+ 本轮消息，
/// 成功后把新 session id 接回本会话。调用方须已完成 begin_*（消息入列、running 就位）。
async fn run_fresh_turn(
    state: &tauri::State<'_, AppState>,
    conv_id: String,
    last_user: String,
    pop_user: bool,
    on_event: Channel<agent::TurnEvent>,
) -> Result<Conversation, String> {
    // 跑起来之前的任何失败都要把 running 收回（见 abort_turn）
    macro_rules! prep {
        ($e:expr) => {
            match $e {
                Ok(v) => v,
                Err(e) => {
                    abort_turn(state, &conv_id, pop_user);
                    return Err(e.into());
                }
            }
        };
    }
    let conv = prep!(state.convos.get(&conv_id).ok_or("会话不存在"));
    let (conn, work_dir) = prep!(conversation_runtime(&state.conns, &state.sites, &state.data_dir, &conv));
    let is_workspace = conv.task_type == "workspace";
    let sys = if conv.task_type == "site" {
        let site = prep!(state.sites.get(&conv.site_slug));
        site_prompt(&site, &conn, &state.data_dir, &state.sites)
    } else {
        conversation_system_prompt(
            &conn,
            is_workspace,
            &conv.workspace_dir,
            &conv.site_slug,
            &work_dir,
            &conv.brain,
            &state.data_dir,
        )
    };
    // 历史摘要（不含将要重跑的最后一条用户消息）；发给模型的组合消息不落库，界面保持干净。
    // 手动重建只在底层会话确实损坏/超限时使用。给新 session 足够的近期记录，
    // 避免旧 8k 字符预算让长对话看起来像被清空；界面中的原始消息始终原样保留。
    let recap = convo::recap(&conv.messages, 64_000);
    let message = if recap.is_empty() {
        last_user
    } else {
        format!(
            "【会话已重建】以下是此前对话的记录，供你衔接上下文；项目/站点里的实际文件与内容以当前现状为准，先查看再动手：\n\n{recap}\n\n——\n【继续执行用户的最新请求】\n{last_user}"
        )
    };
    let session_seed = fresh_session_seed(&conv.brain);
    if conv.brain.starts_with("api:") {
        prep!(state.convos.mutate(&conv_id, now_secs(), |c| c.session_ref = session_seed.clone()));
    }

    let res = agent::run_turn(
        state.runs.clone(),
        conn,
        work_dir,
        conv.brain.clone(),
        conv.model.clone(),
        conv.perm_mode.clone(),
        conv.effort.clone(),
        conv.fast,
        state.data_dir.clone(),
        state.ssh.clone(),
        session_seed,
        true,
        Some(sys),
        turn_message(&conv.task_type, message),
        conv_id.clone(),
        turn_checkpoint::wrap(state.convos.clone(), conv_id.clone(), on_event),
        Default::default(),
    )
    .await;

    let now2 = now_secs();
    let updated = state.convos.mutate(&conv_id, now2, |c| {
        // CLI 只接上成功会话；API 已落盘的失败历史也必须保留，避免重放已执行的工具。
        if session_ref_is_persistable(&c.brain, res.ok, &res.session_ref) {
            c.session_ref = res.session_ref.clone();
        }
        c.status = "idle".into();
        c.messages.retain(|m| !m.partial); // 边跑边存的那份草稿换成正式回复
        c.messages.push(assistant_msg(&res, now2, &c.brain));
        apply_usage(c, &res, &state.data_dir);
    })?;
    updated.ok_or_else(|| "会话丢失".into())
}

/// 按计费折算的 token：缓存读取约为输入价的 1/10，缓存写入约 1.25 倍。
fn billed_tokens(u: &agent::TurnUsage) -> u64 {
    u.input + u.output + u.cache_create + u.cache_create / 4 + u.cache_read / 10
}

fn title_from(message: &str) -> String {
    let message = message.trim();
    // 运营任务的指令以「【托管运营 · 本周计划】你是「站点」…」或「【运营 · 改动】页面标题」开头，取成短标题
    if let Some(rest) = message.strip_prefix("【托管运营 · ") {
        if let Some((kind, tail)) = rest.split_once('】') {
            if let Some(name) = tail.strip_prefix("你是「").and_then(|x| x.split('」').next()) {
                return format!("{kind} · {name}");
            }
        }
    }
    if let Some(rest) = message.strip_prefix("【运营 · ") {
        if let Some((kind, tail)) = rest.split_once('】') {
            let first: String = tail.lines().next().unwrap_or("").trim().chars().take(40).collect();
            if !first.is_empty() {
                return format!("{kind} · {first}");
            }
        }
    }
    let t: String = message.chars().take(30).collect();
    if t.is_empty() {
        "新对话".into()
    } else {
        t
    }
}

#[cfg(any(target_os = "windows", test))]
fn powershell_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod title_from_tests {
    #[test]
    fn ops_titles_are_short() {
        assert_eq!(super::title_from("【托管运营 · 本周计划】你是「iosur.com」（iosur.com）的运营负责人。"), "本周计划 · iosur.com");
        assert_eq!(super::title_from("【运营 · 改动】/zh/fees/ 点击率偏低\n\n证据"), "改动 · /zh/fees/ 点击率偏低");
        assert_eq!(super::title_from("  "), "新对话");
    }
}

#[cfg(test)]
mod brain_login_command_tests {
    use super::{
        is_claude_auth_error, powershell_single_quote, validate_claude_oauth_token,
    };

    #[test]
    fn powershell_cli_path_escapes_single_quotes() {
        assert_eq!(
            powershell_single_quote(r"C:\Users\O'Brien\Claude Code\claude.cmd"),
            r"C:\Users\O''Brien\Claude Code\claude.cmd"
        );
    }

    #[test]
    fn claude_subscription_token_validation_rejects_console_keys_and_truncation() {
        assert!(validate_claude_oauth_token("").is_err());
        assert!(validate_claude_oauth_token("too-short").is_err());
        assert!(validate_claude_oauth_token(
            "sk-ant-api03-this-is-a-console-key-not-a-subscription-token"
        )
        .is_err());
        assert_eq!(
            validate_claude_oauth_token(
                "  sk-ant-oat01-this-is-a-long-enough-subscription-token-value  "
            )
            .unwrap(),
            "sk-ant-oat01-this-is-a-long-enough-subscription-token-value"
        );
    }

    #[test]
    fn claude_401_on_stdout_is_a_credential_failure() {
        assert!(is_claude_auth_error(
            r#"Failed to authenticate. API Error: 401 {"message":"Invalid bearer token"}"#
        ));
        assert!(is_claude_auth_error("Not logged in · Please run /login"));
        assert!(is_claude_auth_error(r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#));
        assert!(is_claude_auth_error("API Error: 401 {}"));
    }

    /// 限流 / 额度 / 普通报错里带 token、auth、login 字样，不是凭据问题，不能让用户去重登。
    #[test]
    fn rate_limit_and_generic_errors_are_not_credential_failures() {
        for text in [
            "API Error: 429 This request would exceed the rate limit for tokens per minute",
            "You've hit your usage limit; resets at 3pm",
            "Error: model claude-opus-4-6 does not support author mode",
            "Login page loaded but failed to fetch model list",
            "API Error: 500 token count exceeded",
        ] {
            assert!(!is_claude_auth_error(text), "{text}");
        }
    }
}

/// 深度验证 Claude 凭据。`claude auth status` 只读本地记录会说谎——桌面端换号后
/// 旧 token 已废，它照样报 loggedIn:true（实测）。真话只能靠一次最小无头调用：
/// 正常账号几秒内答复；凭据失效会挂在 OAuth 提示上（stdin 关了也挂），只能硬超时
/// 并杀掉。返回 true=真能用，false=凭据失效；其它异常（网络/模型档位）报 Err，
/// 不冤枉凭据。调用方负责节流（跑一次是一次真实的最小 haiku 计费）。
#[tauri::command]
async fn brain_verify_claude() -> Result<bool, String> {
    let token = keychain::claude_oauth_token()?;
    verify_claude_credential(token.as_deref()).await
}

async fn verify_claude_credential(token: Option<&str>) -> Result<bool, String> {
    let path = brains::resolve_bin("claude");
    if path.is_empty() {
        return Err("没有找到 claude，请先安装".into());
    }
    let mut cmd = tokio::process::Command::new(&path);
    if let Some(token) = token {
        // 用户在 Pilot 中显式选择订阅 Token 时，必须让它成为本次验证的真实凭据；
        // 否则继承环境中的 AUTH_TOKEN/API_KEY 会按 Claude 优先级盖过它。
        cmd.env_remove("ANTHROPIC_AUTH_TOKEN")
            .env_remove("ANTHROPIC_API_KEY");
        cmd.env("CLAUDE_CODE_OAUTH_TOKEN", token);
    }
    cmd.args(["-p", "reply with exactly: ok", "--model", "haiku"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true); // 超时丢弃 future 时把挂死的子进程收掉
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    match tokio::time::timeout(std::time::Duration::from_secs(25), cmd.output()).await {
        Err(_) => Ok(false), // 挂死在授权提示 = 凭据失效
        Ok(Err(e)) => Err(format!("启动 claude 失败：{e}")),
        Ok(Ok(out)) => {
            if out.status.success() && !out.stdout.is_empty() {
                return Ok(true);
            }
            // Claude Code 不同版本会把 API Error 写到 stdout 或 stderr；两边都必须看。
            let diagnostic = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let err = diagnostic.to_lowercase();
            if is_claude_auth_error(&err) {
                Ok(false)
            } else {
                Err(format!(
                    "验证未通过：{}",
                    err.chars().take(160).collect::<String>()
                ))
            }
        }
    }
}

/// 只认明确的「凭据被拒」标记。以前连 "token"/"auth"/"login" 这种词都算——限流提示
/// （"tokens per minute"）、模型名含 auth 的报错都会被当成「Token 失效」让用户白白重登。
fn is_claude_auth_error(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "api error: 401",
        "status 401",
        "401 unauthorized",
        "invalid bearer token",
        "not logged in",
        "failed to authenticate",
        "invalid api key",
        "authentication_error",
        "invalid authentication credentials",
        "please run /login",
        "oauth token has expired",
        "oauth token revoked",
    ]
    .iter()
    .any(|needle| t.contains(needle))
}

/// 保存 `claude setup-token` 生成的订阅 OAuth Token。只写系统安全存储，不修改
/// shell profile、Claude settings 或任何可导出的 Pilot 配置。
#[tauri::command]
async fn save_claude_oauth_token(token: String) -> Result<(), String> {
    let token = validate_claude_oauth_token(&token)?.to_string();
    match verify_claude_credential(Some(&token)).await {
        Ok(true) => keychain::set_claude_oauth_token(&token),
        Ok(false) => Err(
            "Anthropic 拒绝了这个 Token（401）。请重新运行 claude setup-token 并复制新 Token"
                .into(),
        ),
        Err(error) => Err(format!("Token 尚未保存：{error}")),
    }
}

fn validate_claude_oauth_token(token: &str) -> Result<&str, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("请粘贴 Claude 订阅 Token".into());
    }
    if token.chars().any(char::is_whitespace) {
        return Err("Token 中不能包含空格或换行，请确认复制完整".into());
    }
    if token.starts_with("sk-ant-api") {
        return Err("这里仅支持 Claude Code 订阅 Token，不支持 Anthropic Console API Key".into());
    }
    if token.len() < 32 {
        return Err("Token 过短，请运行 claude setup-token 后复制完整结果".into());
    }
    Ok(token)
}

#[tauri::command]
fn clear_claude_oauth_token() -> Result<(), String> {
    keychain::delete_claude_oauth_token()
}

/// 重新绑定第一步：退出该 CLI 的当前账号（headless 跑 logout，不开终端）。
/// 之后前端接着走 open_brain_login 的授权终端登录新账号。
/// logout 命令失败不算错（多半是本来就没登录）——以随后的状态检测为准。
#[tauri::command]
async fn brain_logout(brain: String) -> Result<(), String> {
    let (bin, args): (&str, &[&str]) = match brain.as_str() {
        "claude" => ("claude", &["auth", "logout"]),
        "codex" => ("codex", &["logout"]),
        "grok" => ("grok", &["logout"]),
        other => return Err(format!("未知的执行引擎: {other}")),
    };
    let path = brains::resolve_bin(bin);
    if path.is_empty() {
        return Err(format!("没有找到 {bin}，请先安装"));
    }
    // 显式“重新绑定”必须先移除 Pilot 注入的订阅 Token；否则它的认证优先于 CLI
    // 浏览器登录，新账号登录成功后实际请求仍会继续使用旧 Token。
    if brain == "claude" {
        keychain::delete_claude_oauth_token()?;
    }
    let mut cmd = tokio::process::Command::new(&path);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：别闪控制台
    }
    let fut = async {
        let child = cmd.spawn().map_err(|e| format!("启动 {bin} 失败：{e}"))?;
        let _ = child.wait_with_output().await;
        Ok::<(), String>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(20), fut)
        .await
        .map_err(|_| format!("{bin} logout 超时"))?
}

/// 打开 Terminal 跑对应 CLI 的登录命令（.command 以 zsh 登录 shell 执行，PATH 完整；
/// claude auth login / codex login 会自动拉起浏览器完成授权）。
#[tauri::command]
fn open_brain_login(app: tauri::AppHandle, brain: String) -> Result<(), String> {
    let login_cmd = match brain.as_str() {
        "claude" => "claude auth login --claudeai",
        "codex" => "codex login",
        "grok" => "grok login",
        other => return Err(format!("未知的执行引擎: {other}")),
    };
    // 登录成功标记：claude 读 --json 的 loggedIn=true，codex 读 "logged in"，
    // grok models 已登录时打印 "You are logged in with …"（未登录是 "You are not authenticated."）。
    let (status_cmd, marker) = match brain.as_str() {
        "claude" => ("claude auth status --json", r#""loggedIn": true"#),
        "grok" => ("grok models", "logged in with"),
        _ => ("codex login status", "logged in"),
    };
    // Windows 上一键安装可能位于 Pilot 私有托管目录；命令行安装也可能与 PATH 中其它
    // Claude 版本并存。授权与状态复查必须固定使用检测/对话同款的绝对路径，不能让新
    // PowerShell 再自行解析到另一份 shim。Codex/Grok 保持原有登录命令不变。
    #[cfg(target_os = "windows")]
    let (login_cmd, status_cmd) = if brain == "claude" {
        let cli = powershell_single_quote(&brains::resolve_bin("claude"));
        (
            format!("& '{cli}' auth login --claudeai"),
            format!("& '{cli}' auth status --json"),
        )
    } else {
        (login_cmd.to_string(), status_cmd.to_string())
    };
    // 系统代理注入：授权终端里的 CLI 只认环境变量（浏览器走系统代理没事，CLI 的登录/OIDC
    // 发现请求会直连超时）。生成脚本时把检测到的代理写进开头并回显一行，便于用户自诊。
    // system_proxy_env 在本进程已带代理环境变量时返回空——那正是启动时注入的系统代理，读回来用。
    let proxy_url = system_proxy_env()
        .iter()
        .find(|(k, _)| k == "HTTPS_PROXY")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("HTTPS_PROXY").ok().filter(|v| !v.is_empty()))
        .or_else(|| std::env::var("https_proxy").ok().filter(|v| !v.is_empty()));
    // 失败提示里的厂商登录域（超时/连接错误时的规则代理引导）。
    let login_domains = match brain.as_str() {
        "claude" => "claude.ai",
        "grok" => "auth.x.ai",
        _ => "auth.openai.com 及 chatgpt.com",
    };
    #[cfg(target_os = "macos")]
    let proxy_block = match &proxy_url {
        Some(u) => format!(
            "export HTTPS_PROXY=\"{u}\" HTTP_PROXY=\"{u}\" ALL_PROXY=\"{u}\"\nexport NO_PROXY=\"localhost,127.0.0.1,::1,.local\"\necho \"已注入代理: {u}\"\necho"
        ),
        None => "echo \"未检测到系统代理\"\necho".to_string(),
    };
    #[cfg(target_os = "windows")]
    let proxy_block = match &proxy_url {
        Some(u) => format!(
            "$env:HTTPS_PROXY='{u}'; $env:HTTP_PROXY='{u}'; $env:ALL_PROXY='{u}'\r\nWrite-Host '已注入代理: {u}'\r\nWrite-Host ''\r\n"
        ),
        None => "Write-Host '未检测到系统代理'\r\nWrite-Host ''\r\n".to_string(),
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (&proxy_url, login_domains);
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("login");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    #[cfg(target_os = "macos")]
    {
        let file = dir.join(format!("{brain}-login.command"));
        let script = format!(
            r#"#!/bin/zsh -il
clear
echo "Cligc Pilot · {brain} 授权登录"
echo "浏览器会自动打开，完成登录后【先回到这个窗口】等它打出结果，再关闭。"
echo
{proxy_block}
if [ -z "$HTTPS_PROXY$https_proxy" ]; then
  _sys_proxy=$(scutil --proxy | awk '/HTTPSEnable : 1/{{e=1}} /HTTPSProxy/{{h=$3}} /HTTPSPort/{{p=$3}} END{{if(e&&h&&p) print h":"p}}')
  if [ -n "$_sys_proxy" ]; then
    export HTTPS_PROXY="http://$_sys_proxy" HTTP_PROXY="http://$_sys_proxy" ALL_PROXY="http://$_sys_proxy"
    export NO_PROXY="localhost,127.0.0.1,::1,.local"
  fi
fi
if [ -n "$HTTPS_PROXY$https_proxy" ]; then
  echo "使用代理: ${{HTTPS_PROXY:-$https_proxy}}"
  echo
fi
{login_cmd}
echo
if {status_cmd} 2>/dev/null | grep -qi '{marker}'; then
  echo "✅ 登录成功！现在可以关闭这个窗口，Cligc Pilot 里的状态灯会自动变绿。"
else
  echo "❌ 登录还没完成。请截图上面的输出，或重新在 Pilot 里点「去授权」再试一次。"
  echo "   若上面是 timed out / connection 错误：代理/VPN 需覆盖 {login_domains}，规则模式请把该域加进规则或临时切全局。"
fi
echo
read -s -k 1 "?按任意键关闭这个窗口…"
"#
        );
        std::fs::write(&file, script).map_err(|e| e.to_string())?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        std::process::Command::new("open")
            .arg(&file)
            .spawn()
            .map_err(|e| format!("打开终端失败: {e}"))?;
    }

    #[cfg(target_os = "windows")]
    {
        // PowerShell：UTF-8 输出 + 注入系统代理（CLI 只认环境变量）+ 跑登录命令 + 状态自查。
        let file = dir.join(format!("{brain}-login.ps1"));
        let script = format!(
            "$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8\r\n\
             Write-Host 'Cligc Pilot · {brain} 授权登录'\r\n\
             Write-Host '浏览器会自动打开，完成登录后先回到这个窗口等它打出结果，再关闭。'\r\n\
             Write-Host ''\r\n\
             {proxy_block}\
             {login_cmd}\r\n\
             Write-Host ''\r\n\
             if ({status_cmd} 2>$null | Select-String -SimpleMatch '{marker}') {{\r\n\
             Write-Host '[OK] 登录成功！现在可以关闭这个窗口，Cligc Pilot 里的状态灯会自动变绿。'\r\n\
             }} else {{\r\n\
             Write-Host '[X] 登录还没完成。请截图上面的输出，或重新在 Pilot 里点「去授权」再试一次。'\r\n\
             Write-Host '    若上面是 timed out / connection 错误：代理/VPN 需覆盖 {login_domains}，规则模式请把该域加进规则或临时切全局。'\r\n\
             }}\r\n\
             Read-Host '按回车关闭这个窗口'\r\n"
        );
        // 必须带 UTF-8 BOM：无 BOM 的 .ps1 会被 Windows PowerShell 5.1 按 ANSI 解析，
        // 中文文本直接造成脚本解析失败——窗口闪一下就退，这就是「去授权闪退」的根因。
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(script.as_bytes());
        std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
        // 用 cmd start 拉起一个可见的 PowerShell 窗口跑这个临时脚本。
        std::process::Command::new("cmd")
            .args([
                "/c",
                "start",
                "",
                "powershell",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&file)
            .spawn()
            .map_err(|e| format!("打开终端失败: {e}"))?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (login_cmd, status_cmd, marker);
        return Err("当前平台暂不支持一键授权".to_string());
    }

    Ok(())
}

/// Windows 11：把原生标题栏背景/文字色设成与左栏（rail）一致，消除白色标题栏与米色侧栏的割裂。
/// Win10 不支持该 DWM 属性，调用失败被忽略（标题栏维持系统默认）。
#[cfg(target_os = "windows")]
fn style_titlebar_windows(w: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
    };
    let Ok(hwnd) = w.hwnd() else { return };
    // COLORREF = 0x00BBGGRR。rail #faf9f7 → 0x00F7F9FA；标题文字 --text #26241f → 0x001F2426。
    let caption = COLORREF(0x00F7F9FA);
    let text = COLORREF(0x001F2426);
    let sz = std::mem::size_of::<COLORREF>() as u32;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &caption as *const _ as *const core::ffi::c_void,
            sz,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            &text as *const _ as *const core::ffi::c_void,
            sz,
        );
    }
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Cligc Pilot", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Cligc Pilot")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => {
                stop_preview(&app.state::<AppState>()); // 退出前杀掉本地预览进程
                app.exit(0);
            }
            _ => {}
        });
    // macOS 菜单栏用单色模板图标（只有「C」字形），由系统按菜单栏深浅自动染色，
    // 与其它菜单栏图标同一风格；应用图标带底色方块和 Dock 边距，缩进菜单栏会显得偏小。
    #[cfg(target_os = "macos")]
    {
        let glyph = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
        builder = builder.icon(glyph).icon_as_template(true);
    }
    // Windows / Linux 的托盘不支持模板图标，黑色字形在深色任务栏上会看不见，继续用应用图标。
    #[cfg(not(target_os = "macos"))]
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// macOS 菜单栏（替换 Tauri 默认的英文菜单）：应用菜单里有「关于」（带应用图标）、「检查更新…」、「设置…」；
/// 另有编辑、视图、窗口三组。自定义项发 `app-menu` 事件给主窗口（update / settings），由前端打开对应界面。
#[cfg(target_os = "macos")]
fn setup_app_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{AboutMetadata, PredefinedMenuItem, Submenu};
    let name = app.package_info().name.clone();
    let about = PredefinedMenuItem::about(
        app,
        Some(&format!("关于 {name}")),
        Some(AboutMetadata { name: Some(name.clone()), icon: app.default_window_icon().cloned(), copyright: Some("© cligc".into()), ..Default::default() }),
    )?;
    let sep = || PredefinedMenuItem::separator(app);
    let update = MenuItem::with_id(app, "app-menu:update", "检查更新…", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "app-menu:settings", "设置…", true, Some("CmdOrCtrl+,"))?;
    let quit = MenuItem::with_id(app, "app-menu:quit", format!("退出 {name}"), true, Some("CmdOrCtrl+Q"))?;
    let app_menu = Submenu::with_items(
        app,
        &name,
        true,
        &[
            &about,
            &update,
            &sep()?,
            &settings,
            &sep()?,
            &PredefinedMenuItem::hide(app, Some(&format!("隐藏 {name}")))?,
            &PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?,
            &PredefinedMenuItem::show_all(app, Some("全部显示"))?,
            &sep()?,
            &quit,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &PredefinedMenuItem::undo(app, Some("撤销"))?,
            &PredefinedMenuItem::redo(app, Some("重做"))?,
            &sep()?,
            &PredefinedMenuItem::cut(app, Some("剪切"))?,
            &PredefinedMenuItem::copy(app, Some("拷贝"))?,
            &PredefinedMenuItem::paste(app, Some("粘贴"))?,
            &PredefinedMenuItem::select_all(app, Some("全选"))?,
        ],
    )?;
    let view = Submenu::with_items(app, "视图", true, &[&PredefinedMenuItem::fullscreen(app, Some("进入全屏幕"))?])?;
    let window = Submenu::with_items(
        app,
        "窗口",
        true,
        &[&PredefinedMenuItem::minimize(app, Some("最小化"))?, &PredefinedMenuItem::maximize(app, Some("缩放"))?, &sep()?, &PredefinedMenuItem::close_window(app, Some("关闭窗口"))?],
    )?;
    app.set_menu(Menu::with_items(app, &[&app_menu, &edit, &view, &window])?)?;
    Ok(())
}

/// 应用菜单里的自定义项：退出前停掉本地预览；其余交给主窗口的前端。
fn on_app_menu(app: &tauri::AppHandle, id: &str) {
    let Some(what) = id.strip_prefix("app-menu:") else { return };
    if what == "quit" {
        stop_preview(&app.state::<AppState>());
        app.exit(0);
        return;
    }
    show_main(app);
    let _ = app.emit_to("main", "app-menu", what);
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // GUI 进程的 PATH / 代理是裸的，必须最先修复，否则找不到 claude/node 且直连被墙。
    path_env::fix();
    apply_system_proxy_to_process();

    tauri::Builder::default()
        .plugin(sftp_drop::plugin())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let _ = APP.set(app.handle().clone());
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            // 托管 Node（若已自举过）：bin + npm 全局 bin 前置进程 PATH——
            // 探测（which 读 PATH）与之后 spawn 的全部 CLI 子进程自动继承。
            node_boot::prepend_process_path(&data_dir);
            let conns = conns::ConnStore::new(&data_dir).map_err(std::io::Error::other)?;
            let convos = convo::ConvStore::new(&data_dir);
            convos.mark_idle(now_secs());
            // 清掉上次进程遗留的待批权限请求（幽灵批准卡）。
            permit::sweep_all(&data_dir.join("permit").join("pending"));
            let _ = tools::ensure_shot(&data_dir); // 随附截图工具，覆写以随版本刷新
            let _ = tools::ensure_ssh(&data_dir); // 随附远程执行工具（AI 桥的 AI 侧）
            let _ = tools::ensure_site_data(&data_dir); // 随附站点数据查询工具（只读本地缓存）
            let _ = tools::ensure_phone(&data_dir); // 随附安卓手机工具（adb）
            let _ = tools::ensure_design_plugin(&data_dir); // 随附建站设计技能（claude/grok 走 --plugin-dir）
            let _ = cf_templates::ensure_builtin(&data_dir); // 随附起始模板，覆写以随版本刷新
            app.manage(AppState {
                conns,
                convos,
                runs: agent::RunRegistry::default(),
                data_dir: data_dir.clone(),
                preview: Arc::new(Mutex::new(None)),
                ssh: ssh::SshSessions::new(),
                sites: sites::SiteStore::new(&data_dir),
                site_jobs: site_jobs::Jobs::default(),
            });
            setup_tray(app.handle())?;
            #[cfg(target_os = "macos")]
            setup_app_menu(app.handle())?;
            site_runner::start_scheduler(app.handle().clone());
            push::start(app.handle().clone());
            // 邮箱验证码的本机端口（phone.cjs 的 mail-* 命令从这里取）
            tauri::async_runtime::spawn(mail::serve(data_dir.clone()));
            // 先恢复上次的窗口大小 / 位置，再显示
            if let Some(w) = app.get_webview_window("main") {
                window_memory::restore(&w, &data_dir);
            }
            // 每次启动都把主窗口显示并置前——修复自动更新 relaunch 后 macOS 不激活、窗口看不到（要点 Dock 才出来）。
            // 开机自动启动（带 --background）除外：只在菜单栏 / 托盘常驻，后台照常运营
            if !std::env::args().any(|a| a == background::BACKGROUND_ARG) {
                show_main(app.handle());
            }
            #[cfg(target_os = "windows")]
            if let Some(w) = app.get_webview_window("main") {
                style_titlebar_windows(&w);
            }
            // 调试包排障：PILOT_DEVTOOLS=1 启动时直接打开主窗口的开发者工具。
            #[cfg(debug_assertions)]
            if std::env::var_os("PILOT_DEVTOOLS").is_some() {
                if let Some(w) = app.get_webview_window("main") {
                    w.open_devtools();
                }
            }
            Ok(())
        })
        .on_menu_event(|app, event| on_app_menu(app, event.id.as_ref()))
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, WindowEvent::Resized(_) | WindowEvent::Moved(_)) {
                if let Some(w) = window.app_handle().get_webview_window("main") {
                    window_memory::schedule_save(&w);
                }
            }
            if matches!(event, WindowEvent::Destroyed) {
                sftp_transfer::cancel_owner(window.label());
                sftp_drop::clear_owner(window.label());
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                } else if window.label() == "preview" {
                    // 关预览窗＝停掉 wrangler dev（否则进程 + 端口泄漏），但让窗口正常关。
                    stop_preview(&window.app_handle().state::<AppState>());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_connections,
            remove_connection,
            set_connection_remark,
            open_conn_window,
            show_edit_menu,
            detect_brains,
            list_api_profiles,
            save_api_profile,
            delete_api_profile,
            list_api_models,
            test_api_profile,
            get_api_billing_settings,
            save_api_billing_key,
            delete_api_billing_key,
            query_api_billing,
            install_wrangler,
            phone_status,
            install_adb,
            mail_status,
            mail_connect,
            mail_remove,
            mail_google_connect,
            mail_fill_code,
            suggest_reply,
            blogger::blogger_login,
            blogger::blogger_blogs,
            wpcom::wpcom_client_info,
            wpcom::wpcom_save_client,
            wpcom::wpcom_login,
            wpcom::wpcom_sites,
            syndicate::syndication_state,
            syndicate::syndication_save_channel,
            syndicate::syndication_remove_channel,
            syndicate::syndication_generate,
            syndicate::syndication_edit,
            syndicate::syndication_publish,
            syndicate::syndication_skip,
            syndicate::syndication_cancel,
            syndicate::syndication_pending,
            syndicate::syndication_reconcile,
            syndicate::syndication_save_settings,
            phone_stay_awake,
            install_claude,
            verify_cf_token,
            connect_cloudflare,
            update_cloudflare_token,
            verify_github_token,
            connect_github,
            update_github_token,
            list_github_repos,
            list_github_checkouts,
            verify_ssh,
            connect_ssh,
            update_ssh,
            ssh_os_probe,
            ssh_stats,
            ssh_open_shell,
            ssh_input,
            ssh_resize,
            ssh_close,
            sftp_list,
            sftp_home,
            sftp_read,
            sftp_write,
            sftp_rename,
            sftp_copy,
            sftp_remove,
            sftp_mkdir,
            sftp_create_file,
            sftp_transfer::sftp_download_entry,
            sftp_transfer::sftp_cancel_download,
            sftp_transfer::sftp_start_drag,
            sftp_transfer::sftp_drag_download,
            sftp_drop::sftp_capture_local_drop,
            sftp_drop::sftp_consume_local_drop,
            sftp_drop::sftp_discard_local_drop,
            sftp_download,
            sftp_upload,
            sftp_transfer::sftp_upload_entry,
            save_attachment,
            save_attachment_path,
            read_workdir_image,
            resolve_workdir_file,
            usage_stats,
            install_codex,
            install_grok,
            list_cf_projects,
            cf_project_ready,
            cf_preview_start,
            cf_preview_stop,
            site_preview_start,
            cf_preview_template,
            list_templates,
            template_index_html,
            save_as_template,
            delete_template,
            use_template,
            redetect_brains,
            open_brain_login,
            brain_logout,
            brain_verify_claude,
            save_claude_oauth_token,
            clear_claude_oauth_token,
            list_conversations,
            get_conversation,
            get_conversation_live,
            delete_conversation,
            start_conversation,
            send_message,
            retry_turn,
            rebuild_session,
            cancel_turn,
            set_conversation_model,
            set_conversation_brain_model,
            set_conversation_perm_mode,
            set_conversation_effort,
            set_conversation_fast,
            check_claude_update,
            upgrade_claude_cli,
            list_pending_permits,
            respond_permit,
            site_cmds::detect_site_folder,
            site_cmds::detect_site_repo,
            site_cmds::list_cf_pages_projects,
            site_cmds::detect_cf_deploy,
            site_cmds::autodetect_cf_deploys,
            site_cmds::list_sites,
            site_cmds::import_site,
            site_cmds::create_site,
            site_cmds::site_standards,
            migrate_cmds::create_migration,
            migrate_cmds::create_migrations,
            migrate_cmds::migrate_state,
            migrate_cmds::migrate_survey,
            migrate_cmds::migrate_save_plan,
            migrate_cmds::migrate_extract,
            migrate_cmds::migrate_rebuild_message,
            migrate_cmds::migrate_verify,
            migrate_cmds::migrate_live,
            migrate_cmds::migrate_directions_message,
            migrate_cmds::migrate_directions,
            migrate_cmds::migrate_pick_direction,
            migrate_cmds::migrate_set_auto,
            migrate_cmds::migrate_direction_open,
            migrate_cmds::migrate_set_formal,
            migrate_cmds::migrate_host_use,
            migrate_cmds::migrate_old_redirect,
            migrate_cmds::migrate_old_redirect_undo,
            migrate_cmds::migrate_staging,
            migrate_cmds::migrate_staging_remove,
            migrate_cmds::migrate_staging_check,
            migrate_cmds::migrate_staging_status,
            migrate_cmds::migrate_ownership,
            migrate_cmds::migrate_dns_prepare,
            migrate_cmds::migrate_dns_fix,
            migrate_cmds::migrate_cutover,
            migrate_cmds::migrate_rollback,
            migrate_cmds::migrate_track,
            migrate_cmds::migrate_hosting,
            migrate_cmds::migrate_vps_dns,
            site_dynamic::site_dynamic_state,
            site_dynamic::site_dynamic_features,
            site_dynamic::site_dynamic_setup,
            site_dynamic::site_inbox,
            site_dynamic::site_inbox_act,
            site_dynamic::site_dynamic_message,
            site_vps_host::site_vps_setup,
            site_vps_host::site_vps_activate,
            site_vps_host::site_vps_state,
            site_cmds::site_make_state,
            site_cmds::site_continue_message,
            site_cmds::discard_made_site,
            site_cmds::site_todo_handled,
            site_cmds::site_todo_request_message,
            site_cmds::site_config_state,
            site_cmds::site_config_sync,
            site_cmds::site_config_pull,
            site_cmds::site_config_reveal,
            site_cmds::site_config_sync_one,
            site_cmds::save_site,
            site_cmds::remove_site,
            site_cmds::site_takedown,
            site_cmds::site_env_keys,
            site_cmds::set_site_env,
            site_cmds::site_status,
            site_cmds::site_icons,
            site_cmds::site_stage_info,
            site_cmds::save_site_positioning,
            site_cmds::site_deploys,
            site_cmds::site_job,
            site_cmds::cancel_site_job,
            site_cmds::deploy_site,
            site_cmds::check_site,
            site_cmds::site_pages_deployments,
            site_cmds::retry_pages_deployment,
            site_cmds::rollback_pages_deployment,
            site_cmds::vps_key_status,
            site_cmds::setup_vps_key,
            site_cmds::revoke_vps_key,
            data_cmds::verify_google_sa,
            data_cmds::connect_google_sa,
            data_cmds::update_google_sa,
            data_cmds::list_google_resources,
            data_cmds::autodetect_data_bindings,
            data_cmds::verify_bing_key,
            data_cmds::connect_bing,
            data_cmds::update_bing_key,
            data_cmds::list_bing_sites,
            data_cmds::refresh_site_data,
            data_cmds::site_dashboard,
            data_cmds::setup_indexnow,
            data_cmds::read_key_file,
            data_cmds::google_oauth_login,
            data_cmds::google_oauth_client,
            data_cmds::save_google_oauth_client,
            data_cmds::google_oauth_cancel,
            data_cmds::google_can_manage,
            ops_cmds::site_queue,
            ops_cmds::queue_add,
            ops_cmds::run_ops_task,
            ops_cmds::queue_work,
            ops_cmds::queue_recheck,
            ops_cmds::queue_work_many,
            ops_cmds::queue_ack,
            ops_cmds::queue_accept,
            ops_cmds::queue_rollback,
            ops_cmds::queue_dismiss,
            ops_cmds::queue_restore,
            ops_cmds::queue_mark_done,
            ops_cmds::queue_diff,
            ops_cmds::ops_overview,
            ops_cmds::list_reports,
            ops_cmds::read_report,
            launch_cmds::import_cf_project,
            launch_cmds::launch_status,
            launch_cmds::launch_auto_bind,
            typesafe::typesafe_status,
            typesafe::typesafe_save,
            typesafe::typesafe_test,
            typesafe::typesafe_remove,
            launch_cmds::site_to_github,
            launch_cmds::ga4_to_config,
            launch_cmds::site_pages_dns_replace,
            launch_cmds::site_domain_check,
            launch_cmds::site_domain_fix,
            launch_cmds::site_bing_verify,
            site_cmds::site_publish_files,
            launch_cmds::site_setup_pages,
            launch_cmds::ga4_create_property,
            launch_cmds::gsc_setup_domain,
            launch_cmds::ga4_accounts,
            fleet_cmds::sites_overview,
            fleet_cmds::pending_across_sites,
            fleet_cmds::recent_ships,
            fleet_cmds::site_links,
            fleet_cmds::check_site_links,
            fleet_cmds::site_monitor,
            fleet_cmds::check_site_monitor,
            background::background_status,
            background::background_set,
            background::background_dismiss_notice,
            backup::backup_status,
            push::push_status,
            push::push_set_token,
            push::push_pair,
            push::push_set,
            push::push_test,
            push::push_forget,
            revenue::revenue_status,
            revenue::adsense_connect,
            revenue::adsense_refresh,
            revenue::adsense_disconnect,
            revenue::site_revenue,
            revenue::revenue_add,
            revenue::revenue_remove,
            revenue::revenue_import,
            speed::site_speed,
            speed::check_site_speed,
            serp::site_serp,
            serp::set_site_serp,
            serp::check_site_serp,
            serp::api_keys_status,
            serp::api_key_set,
            serp::api_key_remove,
            page_audit::site_audit,
            page_audit::check_site_audit,
            backup::backup_set,
            backup::backup_now,
            backup::backup_preview,
            backup::backup_restore,
            fleet_cmds::set_site_flags,
            fleet_cmds::opportunities_across_sites,
            fleet_cmds::work_across_sites,
            fleet_cmds::data_sources_overview,
            fleet_cmds::auto_bind_sites,
            managed_cmds::managed_config,
            managed_cmds::save_managed_config,
            managed_cmds::set_managed_paused,
            managed_cmds::set_sites_managed,
            managed_cmds::save_managed_group,
            managed_cmds::delete_managed_group,
            managed_cmds::set_group_paused,
            managed_cmds::set_site_managed_active,
            managed_cmds::managed_overview,
            managed_cmds::managed_activity,
            managed_cmds::managed_plans,
            managed_cmds::list_digests,
            managed_cmds::managed_daily,
            managed_cmds::managed_site_daily,
            managed_cmds::plan_clear,
            managed_cmds::plan_remove_task,
            managed_cmds::read_digest,
            managed_cmds::read_digest_data,
            managed_cmds::digest_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
