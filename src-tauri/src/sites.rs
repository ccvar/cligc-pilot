//! 站点：连接之上的运营对象。站点只记录引用——代码在哪（GitHub 仓库 / 本地文件夹）、
//! 部署到哪个 Cloudflare Pages 项目、对应哪些数据源——凭据始终归连接所有。
//! 元数据存 sites.json（原子写 + 互斥），每站一个数据目录 `<data>/sites/<id>/`
//! （数据缓存、部署记录、运营队列、报告都在里面，不进代码仓库）。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct BuildConfig {
    /// 站点在仓库里的子目录（如 `site`）；空＝仓库根目录。安装、构建、检查、部署都在这个目录里执行。
    #[serde(default)]
    pub root: String,
    /// 安装依赖（如 `npm ci`）；空＝不需要。
    #[serde(default)]
    pub install: String,
    /// 构建命令（如 `npm run build`）；空＝纯静态站，直接上传 out_dir。
    #[serde(default)]
    pub build: String,
    /// 构建产物目录（相对站点根），如 `dist`；纯静态站为 `.`。
    #[serde(default)]
    pub out_dir: String,
    /// 构建后、上线前必须通过的检查命令（如 `npm run check`）。任何一条失败都不上传。
    #[serde(default)]
    pub checks: Vec<String>,
    /// 构建时变量的名字（值在钥匙串 `site-env:<id>`，只注入 Pilot 执行的构建）。
    #[serde(default)]
    pub env_keys: Vec<String>,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct DeployConfig {
    /// direct（Pilot 构建后用 wrangler 上传到 Pages）| workers（Pilot 构建后 `wrangler deploy` 到 Workers，
    /// 配置在仓库的 wrangler.toml / wrangler.jsonc 里）| pages_git（Pages 绑定 GitHub，合并即上线）| none（暂不部署）
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub cf_conn_id: String,
    /// Cloudflare Pages 项目名；Workers 部署时是 Worker 名（来自 wrangler 配置）。
    #[serde(default)]
    pub project: String,
    /// 生产分支（Pages 的 production branch）。
    #[serde(default)]
    pub branch: String,
    /// VPS 部署：用哪条 SSH 连接授权（Pilot 专用部署密钥加在这台服务器上）。
    #[serde(default)]
    pub ssh_conn_id: String,
    /// VPS 部署：项目自带的发布命令（在站点根目录执行），如 `./scripts/publish-production.sh publish`。
    #[serde(default)]
    pub command: String,
    /// VPS 部署：服务器上的配置变量文件（Pilot 部署时把站点配置变量合并写进去）。空＝/opt/<站点>/config/<站点>.env。
    #[serde(default)]
    pub env_file: String,
}

impl DataBinding {
    /// 实际用来读 Search Console 的连接。
    pub fn gsc_conn(&self) -> &str {
        if self.gsc_conn_id.is_empty() { &self.google_conn_id } else { &self.gsc_conn_id }
    }
}

/// 先写到旁边的临时文件再改名：写到一半崩了（断电、强退）原文件还是完整的，不会留下半截 JSON。
pub fn write_atomic(p: &Path, raw: &[u8]) -> Result<(), String> {
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = p.with_file_name(format!("{name}.tmp"));
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}

/// 把一个值序列化成 JSON 原子写入（`pretty`＝带缩进）。
pub fn write_json<T: serde::Serialize>(p: &Path, v: &T, pretty: bool) -> Result<(), String> {
    let raw = if pretty { serde_json::to_vec_pretty(v) } else { serde_json::to_vec(v) }.map_err(|e| e.to_string())?;
    write_atomic(p, &raw)
}

impl Site {
    /// 站点自己的目录（代码目录下的 build.root 子目录；没有子目录就是代码目录本身）。
    pub fn app_dir(&self, code: &Path) -> std::path::PathBuf {
        let r = self.build.root.trim().trim_matches('/');
        if r.is_empty() || r == "." { code.to_path_buf() } else { code.join(r) }
    }
}

/// 子目录只能是仓库里的相对路径（不能跳出仓库）。
pub fn check_root(root: &str) -> Result<(), String> {
    let r = root.trim();
    if r.is_empty() {
        return Ok(());
    }
    if r.starts_with('/') || r.starts_with('\\') || r.contains("..") || r.contains(':') {
        return Err("站点子目录只能填仓库里的相对路径，如 site".into());
    }
    Ok(())
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct DataBinding {
    #[serde(default)]
    pub google_conn_id: String,
    /// GA4 媒体资源，形如 `properties/123456789`。
    #[serde(default)]
    pub ga4_property: String,
    #[serde(default)]
    pub ga4_property_name: String,
    /// Search Console 资源：`sc-domain:example.com` 或 `https://example.com/`。
    #[serde(default)]
    pub gsc_site: String,
    /// Search Console 用的 Google 连接。GA4 和 Search Console 可能在不同的 Google 账号下；
    /// 留空表示和 GA4 用同一条（`google_conn_id`），老数据都是这样。
    #[serde(default)]
    pub gsc_conn_id: String,
    #[serde(default)]
    pub bing_conn_id: String,
    /// Bing Webmaster 里的站点网址（形如 `https://example.com/`）。
    #[serde(default)]
    pub bing_site: String,
    /// Pilot 自动加到 Bing 时拿到的验证码（BingSiteAuth.xml 的内容）：验证通过前上线会把验证文件放进构建产物。
    /// 有它、`bing_site` 还空着 = 已加到 Bing、等验证。
    #[serde(default)]
    pub bing_auth: String,
    /// IndexNow 验证密钥（站点根目录下 `<key>.txt` 的内容）。
    #[serde(default)]
    pub indexnow_key: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct OpsBudget {
    /// 每周最多产出多少条改动（方案 / PR / 自动上线合计）。
    #[serde(default = "default_changes_per_week")]
    pub changes_per_week: u32,
    /// 每次运行最多处理几个机会。
    #[serde(default = "default_items_per_run")]
    pub items_per_run: u32,
    /// 每周最多消耗多少 token（模型用量上限）；0＝不限。
    #[serde(default = "default_tokens_per_week")]
    pub tokens_per_week: u64,
}

fn default_changes_per_week() -> u32 {
    10
}
fn default_items_per_run() -> u32 {
    3
}
fn default_tokens_per_week() -> u64 {
    3_000_000
}

impl Default for OpsBudget {
    fn default() -> Self {
        Self {
            changes_per_week: default_changes_per_week(),
            items_per_run: default_items_per_run(),
            tokens_per_week: default_tokens_per_week(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct OpsConfig {
    /// 自动化档位：1 只分析 | 2 出方案 | 3 自动提 PR | 4 低风险自动上线。默认 2。
    #[serde(default = "default_level")]
    pub level: u8,
    /// 第 4 档白名单：meta_description | title | internal_links | sitemap。默认全部不勾。
    #[serde(default)]
    pub whitelist: Vec<String>,
    /// 各任务的周期：off | daily | weekly。
    #[serde(default = "default_weekly")]
    pub scan: String,
    #[serde(default = "default_weekly")]
    pub health: String,
    #[serde(default = "default_weekly")]
    pub report: String,
    #[serde(default = "default_weekly")]
    pub review: String,
    #[serde(default)]
    pub budget: OpsBudget,
    /// 无人值守任务用的模型服务与模型（不允许 codex：它没有逐条确认钩子）。
    #[serde(default = "default_brain")]
    pub brain: String,
    #[serde(default)]
    pub model: String,
    /// 效果复盘持续负面而被自动降档的改动类型（这些类型在该站最高只到第 3 档）。
    #[serde(default)]
    pub downgraded: Vec<String>,
    /// 机会扫描阈值：展现量下限（小站可调低）。
    #[serde(default = "default_min_impressions")]
    pub min_impressions: u32,
    /// 托管运营用的站点阶段：空＝按数据自动判断；new / growing / established＝运营人员手动指定。
    #[serde(default)]
    pub stage: String,
}

/// 新导入的站点默认自己运营（第 1 档）：照常同步数据、找机会、体检，不自动改代码；要自动运营就加入托管方案。
fn default_level() -> u8 {
    1
}
fn default_weekly() -> String {
    "weekly".into()
}
fn default_brain() -> String {
    "claude".into()
}
fn default_min_impressions() -> u32 {
    100
}

impl Default for OpsConfig {
    fn default() -> Self {
        Self {
            level: default_level(),
            whitelist: Vec::new(),
            scan: default_weekly(),
            health: default_weekly(),
            report: default_weekly(),
            review: default_weekly(),
            budget: OpsBudget::default(),
            brain: default_brain(),
            model: String::new(),
            downgraded: Vec::new(),
            min_impressions: default_min_impressions(),
            stage: String::new(),
        }
    }
}

/// 规范化标签：去空白、去重、限长，最多 12 个。
pub fn normalize_tags(tags: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = t.trim().trim_start_matches('#').trim();
        if t.is_empty() {
            continue;
        }
        if t.chars().count() > 20 || t.chars().any(char::is_control) {
            return Err(format!("标签「{t}」太长或含非法字符"));
        }
        if !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    if out.len() > 12 {
        return Err("每个站点最多 12 个标签".into());
    }
    Ok(out)
}

pub const WHITELIST_KINDS: &[&str] = &["meta_description", "title", "internal_links", "sitemap"];

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Site {
    pub id: String,
    pub name: String,
    /// 主域名（不带协议），如 `gatesor.com`。
    #[serde(default)]
    pub domain: String,
    /// github | local
    pub source: String,
    #[serde(default)]
    pub gh_conn_id: String,
    /// owner/repo。GitHub 来源必填；本地来源若有 GitHub 远程也记下（只作展示）。
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub local_dir: String,
    /// 代码的默认分支（PR 的目标分支）。
    #[serde(default)]
    pub default_branch: String,
    #[serde(default)]
    pub framework: String,
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub deploy: DeployConfig,
    #[serde(default)]
    pub data: DataBinding,
    #[serde(default)]
    pub ops: OpsConfig,
    /// 置顶（站点多的时候固定在列表最上面）。
    #[serde(default)]
    pub pinned: bool,
    /// 标签（分组、筛选用）。
    #[serde(default)]
    pub tags: Vec<String>,
    /// created：在 Pilot 里从零制作的；空＝导入的。还没配部署的新站在列表里标「制作中」。
    #[serde(default)]
    pub origin: String,
    /// 迁移中的站点：旧站的域名。迁移期间 `domain` 留空（预览部署不碰线上域名），切换时才填上。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub migrate_from: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone)]
pub struct SiteStore {
    file: PathBuf,
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl SiteStore {
    pub fn new(data_dir: &Path) -> Self {
        let root = data_dir.join("sites");
        let _ = fs::create_dir_all(&root);
        Self {
            file: data_dir.join("sites.json"),
            root,
            lock: Arc::new(Mutex::new(())),
        }
    }

    fn read(&self) -> Vec<Site> {
        crate::store_file::read_or_quarantine(&self.file)
    }

    fn write(&self, sites: &[Site]) -> Result<(), String> {
        let raw = serde_json::to_vec_pretty(sites).map_err(|e| e.to_string())?;
        let tmp = self.file.with_extension("json.tmp");
        fs::write(&tmp, &raw).map_err(|e| format!("写入站点列表失败: {e}"))?;
        fs::rename(&tmp, &self.file).map_err(|e| format!("写入站点列表失败: {e}"))
    }

    pub fn list(&self) -> Vec<Site> {
        let _g = self.lock.lock().unwrap();
        self.read()
    }

    pub fn get(&self, id: &str) -> Result<Site, String> {
        self.list()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("站点不存在（{id}）"))
    }

    /// 站点的本地数据目录（缓存 / 部署记录 / 运营队列 / 报告）。
    pub fn data_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    pub fn insert(&self, mut site: Site) -> Result<Site, String> {
        let _g = self.lock.lock().unwrap();
        let mut all = self.read();
        if site.source == "github" {
            if let Some(dup) = all.iter().find(|s| s.source == "github" && s.repo == site.repo) {
                return Err(format!("仓库 {} 已经导入为站点「{}」", site.repo, dup.name));
            }
        }
        if site.source == "local" {
            if let Some(dup) = all.iter().find(|s| s.source == "local" && s.local_dir == site.local_dir) {
                return Err(format!("这个文件夹已经导入为站点「{}」", dup.name));
            }
        }
        if site.id.is_empty() {
            site.id = uuid::Uuid::new_v4().to_string();
        }
        fs::create_dir_all(self.root.join(&site.id)).map_err(|e| format!("创建站点数据目录失败: {e}"))?;
        all.push(site.clone());
        self.write(&all)?;
        Ok(site)
    }

    /// 在锁内读改写一个站点。`f` 返回 Err 时不落盘。
    pub fn update<F>(&self, id: &str, now: u64, f: F) -> Result<Site, String>
    where
        F: FnOnce(&mut Site) -> Result<(), String>,
    {
        let _g = self.lock.lock().unwrap();
        let mut all = self.read();
        let slot = all.iter_mut().find(|s| s.id == id).ok_or("站点不存在")?;
        let mut next = slot.clone();
        f(&mut next)?;
        next.id = slot.id.clone();
        next.created_at = slot.created_at;
        next.updated_at = now;
        *slot = next.clone();
        self.write(&all)?;
        Ok(next)
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let _g = self.lock.lock().unwrap();
        let mut all = self.read();
        let before = all.len();
        all.retain(|s| s.id != id);
        if all.len() == before {
            return Err("站点不存在".into());
        }
        self.write(&all)?;
        // 只删 Pilot 自己的数据目录；用户的代码（本地文件夹 / GitHub 检出）一律不动。
        let _ = fs::remove_dir_all(self.root.join(id));
        Ok(())
    }
}

/// 校验并规范化用户填写的域名：去协议、去路径、转小写；只允许常见主机名字符。
pub fn normalize_domain(raw: &str) -> Result<String, String> {
    let mut s = raw.trim().to_ascii_lowercase();
    for p in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest.to_string();
        }
    }
    let s = s.split(['/', '?', '#']).next().unwrap_or("").trim_end_matches('.').to_string();
    if s.is_empty() {
        return Ok(String::new());
    }
    let ok = s.len() <= 253
        && s.contains('.')
        && s.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if ok {
        Ok(s)
    } else {
        Err(format!("域名格式不正确：{raw}"))
    }
}

/// 命令字段只允许单行：换行会让「一条命令」在 shell 里变成多条，界面上看不出来。
pub fn check_command(cmd: &str) -> Result<(), String> {
    if cmd.contains('\n') || cmd.contains('\r') {
        return Err("命令只能写一行".into());
    }
    if cmd.chars().count() > 400 {
        return Err("命令太长".into());
    }
    Ok(())
}

/// 构建产物目录必须是站点根下的相对路径，不允许 `..` 跳出去（否则会把别处的文件传上线）。
pub fn check_out_dir(dir: &str) -> Result<(), String> {
    let p = Path::new(dir.trim());
    if dir.trim().is_empty() {
        return Err("请填写构建产物目录（纯静态站填 .）".into());
    }
    if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("构建产物目录必须是站点内的相对路径".into());
    }
    Ok(())
}

/// 构建时变量名：只允许常见的环境变量命名。
pub fn check_env_key(key: &str) -> Result<(), String> {
    let ok = !key.is_empty()
        && key.len() <= 64
        && key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!("变量名不合法：{key}"))
    }
}

// ---------------------------------------------------------------------------
// 导入时的项目识别（纯函数，只读文件系统）
// ---------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Debug, PartialEq)]
pub struct Detected {
    pub framework: String,
    pub build: BuildConfig,
    /// 从 deploy 脚本里解析出的构建时变量（如 `GA_MEASUREMENT_ID=G-XXXX npm run build`）。
    pub env: Vec<(String, String)>,
    pub cf_project: String,
    /// pages | workers | vps：deploy 脚本 / wrangler 配置 / 发布脚本显示的部署方式。
    pub deploy_kind: String,
    /// VPS：识别出的发布命令。
    pub deploy_command: String,
    /// VPS：发布脚本 / publish.env 里写的默认目标（user@host），用来自动选上对应的 SSH 连接。
    pub deploy_target: String,
    pub branch: String,
    pub domain: String,
    pub indexnow_key: String,
    /// 本地 git 信息。
    pub git: bool,
    pub git_branch: String,
    pub git_default_branch: String,
    pub github_repo: String,
    /// 项目自带 `.env` 但不在仓库里（GitHub 来源重新克隆后要提示补齐）。
    pub has_dotenv: bool,
    pub dotenv_example_keys: Vec<String>,
    pub notes: Vec<String>,
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn has_dep(pkg: &serde_json::Value, name: &str) -> bool {
    ["dependencies", "devDependencies"]
        .iter()
        .any(|k| pkg.get(k).and_then(|d| d.get(name)).is_some())
}

/// 包管理器：按锁文件判断，决定安装与运行脚本的命令。
fn package_manager(dir: &Path) -> &'static str {
    if dir.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if dir.join("yarn.lock").exists() {
        "yarn"
    } else if dir.join("bun.lockb").exists() || dir.join("bun.lock").exists() {
        "bun"
    } else {
        "npm"
    }
}

fn install_cmd(dir: &Path) -> String {
    match package_manager(dir) {
        "pnpm" => "pnpm install --frozen-lockfile".into(),
        "yarn" => "yarn install --frozen-lockfile".into(),
        "bun" => "bun install --frozen-lockfile".into(),
        _ if dir.join("package-lock.json").exists() => "npm ci".into(),
        _ => "npm install".into(),
    }
}

fn run_script(dir: &Path, script: &str) -> String {
    match package_manager(dir) {
        "pnpm" => format!("pnpm run {script}"),
        "yarn" => format!("yarn {script}"),
        "bun" => format!("bun run {script}"),
        _ => format!("npm run {script}"),
    }
}

/// 解析 deploy 脚本里形如
/// `A=1 B=2 npm run build && wrangler pages deploy dist --project-name x --branch main` 的写法。
/// 返回（前置变量、wrangler 的产物目录、项目名、分支）。认不出就全空，交给其它规则兜底。
pub fn parse_deploy_script(script: &str) -> (Vec<(String, String)>, String, String, String) {
    let mut env = Vec::new();
    let mut out = String::new();
    let mut project = String::new();
    let mut branch = String::new();
    for part in script.split("&&") {
        let words: Vec<&str> = part.split_whitespace().collect();
        let mut i = 0;
        while i < words.len() {
            if let Some((k, v)) = words[i].split_once('=') {
                if check_env_key(k).is_ok() && !k.starts_with("--") {
                    let v = v.trim_matches(['"', '\'']);
                    if !env.iter().any(|(ek, _): &(String, String)| ek == k) {
                        env.push((k.to_string(), v.to_string()));
                    }
                    i += 1;
                    continue;
                }
            }
            break;
        }
        let rest = &words[i..];
        if let Some(pos) = rest.windows(3).position(|w| {
            w[0].ends_with("wrangler") && w[1] == "pages" && w[2] == "deploy"
        }) {
            let args = &rest[pos + 3..];
            let mut j = 0;
            while j < args.len() {
                let a = args[j];
                if let Some(v) = a.strip_prefix("--project-name=") {
                    project = v.into();
                } else if a == "--project-name" {
                    project = args.get(j + 1).copied().unwrap_or_default().into();
                    j += 1;
                } else if let Some(v) = a.strip_prefix("--branch=") {
                    branch = v.into();
                } else if a == "--branch" {
                    branch = args.get(j + 1).copied().unwrap_or_default().into();
                    j += 1;
                } else if a.starts_with("--") {
                    // 其它开关（--commit-dirty=true 等）忽略
                } else if out.is_empty() {
                    out = a.into();
                }
                j += 1;
            }
        }
    }
    (env, out, project, branch)
}

/// 读 wrangler 配置（toml / json / jsonc）：返回（Worker 名，是不是 Workers 项目，自定义路由的主机名）。
/// 有 `main` 或 `assets` 而没有 `pages_build_output_dir` 的就是 Workers。
pub fn wrangler_config(dir: &Path) -> Option<(String, bool, Vec<String>)> {
    if let Ok(t) = fs::read_to_string(dir.join("wrangler.toml")) {
        let mut name = String::new();
        let mut workers = false;
        let mut pages = false;
        let mut hosts = Vec::new();
        let mut top = true;
        for line in t.lines() {
            let l = line.trim();
            if l.starts_with('#') {
                continue;
            }
            if l.starts_with('[') {
                top = false;
                if l.starts_with("[assets]") {
                    workers = true;
                }
                continue;
            }
            let val = |k: &str| l.strip_prefix(k).map(|r| r.trim_start()).and_then(|r| r.strip_prefix('=')).map(|r| r.trim().trim_matches('"').to_string());
            if top {
                if let Some(v) = val("name") {
                    name = v;
                }
                if val("main").is_some() {
                    workers = true;
                }
                if val("pages_build_output_dir").is_some() {
                    pages = true;
                }
            }
            for part in l.split("pattern").skip(1) {
                let v: String = part.trim_start_matches([' ', '=', '"']).chars().take_while(|c| *c != '"').collect();
                if let Ok(h) = normalize_domain(v.trim_end_matches("/*")) {
                    if !h.is_empty() {
                        hosts.push(h);
                    }
                }
            }
        }
        if !name.is_empty() {
            return Some((name, workers && !pages, hosts));
        }
    }
    for f in ["wrangler.jsonc", "wrangler.json"] {
        let Ok(t) = fs::read_to_string(dir.join(f)) else { continue };
        // 去掉 // 注释（不在字符串里的）和尾逗号，再按 JSON 解析
        let mut clean = String::new();
        for line in t.lines() {
            let mut in_str = false;
            let mut prev = ' ';
            let mut cut = line.len();
            for (i, c) in line.char_indices() {
                if c == '"' && prev != '\\' {
                    in_str = !in_str;
                }
                if !in_str && c == '/' && prev == '/' {
                    cut = i - 1;
                    break;
                }
                prev = c;
            }
            clean.push_str(&line[..cut]);
            clean.push('\n');
        }
        let clean = clean.replace(",\n}", "\n}").replace(",\n]", "\n]");
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&clean) else { continue };
        let name = v["name"].as_str().unwrap_or_default().to_string();
        if name.is_empty() {
            continue;
        }
        let workers = (v.get("main").is_some() || v.get("assets").is_some()) && v.get("pages_build_output_dir").is_none();
        let mut hosts = Vec::new();
        for r in v["routes"].as_array().cloned().unwrap_or_default() {
            let p = r.as_str().map(str::to_string).or_else(|| r["pattern"].as_str().map(str::to_string)).unwrap_or_default();
            if let Ok(h) = normalize_domain(p.trim_end_matches("/*")) {
                if !h.is_empty() {
                    hosts.push(h);
                }
            }
        }
        return Some((name, workers, hosts));
    }
    None
}

/// 在脚本文本里找第一个 `user@host` 形式的 SSH 目标（host 为 IP 或域名）。
pub fn find_ssh_target(text: &str) -> Option<String> {
    for raw in text.split(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | '=' | '{' | '}' | '(' | ')' | ':' | ',')) {
        let raw = raw.trim_start_matches('-');
        let Some((user, host)) = raw.split_once('@') else { continue };
        let user_ok = !user.is_empty() && user.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
        let host_ok = host.contains('.') && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')) && !host.ends_with('.');
        if user_ok && host_ok && !host.contains("example") {
            return Some(format!("{user}@{host}"));
        }
    }
    None
}

/// 从 astro/next 等配置里找 `site: 'https://example.com'` 这类声明的域名。
fn domain_from_configs(dir: &Path) -> Option<String> {
    let names = [
        "astro.config.mjs",
        "astro.config.ts",
        "astro.config.js",
        "astro.config.mts",
        "svelte.config.js",
        "nuxt.config.ts",
        "gatsby-config.js",
        "hugo.toml",
        "config.toml",
        "_config.yml",
    ];
    for n in names {
        let Ok(text) = fs::read_to_string(dir.join(n)) else {
            continue;
        };
        for line in text.lines() {
            let l = line.trim();
            let key_hit = ["site:", "site =", "siteUrl:", "baseURL", "url:"]
                .iter()
                .any(|k| l.starts_with(k));
            if !key_hit {
                continue;
            }
            if let Some(start) = l.find("https://") {
                let url: String = l[start..]
                    .chars()
                    .take_while(|c| !matches!(c, '"' | '\'' | '`' | ',' | ' '))
                    .collect();
                if let Ok(d) = normalize_domain(&url) {
                    if !d.is_empty() {
                        return Some(d);
                    }
                }
            }
        }
    }
    None
}

fn domain_from_files(dir: &Path) -> Option<String> {
    for p in ["public/CNAME", "static/CNAME", "CNAME"] {
        if let Ok(t) = fs::read_to_string(dir.join(p)) {
            if let Ok(d) = normalize_domain(t.lines().next().unwrap_or("")) {
                if !d.is_empty() {
                    return Some(d);
                }
            }
        }
    }
    for p in ["public/robots.txt", "static/robots.txt", "robots.txt"] {
        if let Ok(t) = fs::read_to_string(dir.join(p)) {
            for line in t.lines() {
                if let Some(rest) = line.trim().strip_prefix("Sitemap:") {
                    if let Ok(d) = normalize_domain(rest.trim()) {
                        if !d.is_empty() {
                            return Some(d);
                        }
                    }
                }
            }
        }
    }
    None
}

/// IndexNow 验证文件：`<key>.txt`，内容就是 key（8–128 位字母数字或连字符）。
pub fn find_indexnow_key(dir: &Path, out_dir: &str) -> Option<String> {
    let mut cands = vec![dir.join("public"), dir.join("static"), dir.to_path_buf()];
    if !out_dir.is_empty() && out_dir != "." {
        cands.push(dir.join(out_dir));
    }
    for d in cands {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".txt") else { continue };
            if !is_indexnow_key(stem) {
                continue;
            }
            if fs::read_to_string(e.path()).is_ok_and(|t| t.trim() == stem) {
                return Some(stem.to_string());
            }
        }
    }
    None
}

pub fn is_indexnow_key(k: &str) -> bool {
    (8..=128).contains(&k.len()) && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// 解析 git 远程地址里的 GitHub owner/repo。
pub fn github_repo_from_url(url: &str) -> Option<String> {
    let u = url.trim();
    let rest = u
        .strip_prefix("https://github.com/")
        .or_else(|| u.strip_prefix("http://github.com/"))
        .or_else(|| u.strip_prefix("git@github.com:"))
        .or_else(|| u.strip_prefix("ssh://git@github.com/"))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    crate::github::parse_repo(rest).ok()?;
    Some(rest.to_string())
}

pub fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn dotenv_keys(path: &Path) -> Vec<String> {
    let Ok(t) = fs::read_to_string(path) else {
        return Vec::new();
    };
    t.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let k = l.strip_prefix("export ").unwrap_or(l).split('=').next()?.trim();
            check_env_key(k).ok().map(|_| k.to_string())
        })
        .collect()
}

/// 识别项目：框架、构建命令、产物目录、检查命令、Pages 项目、域名、IndexNow 密钥、git 信息。
/// 常见的「站点在子目录里」的仓库布局：根目录没有站点文件，而这些子目录之一有。
const SITE_SUBDIRS: &[&str] = &["site", "web", "www", "app", "frontend", "website", "apps/web", "apps/site"];

fn looks_like_site(dir: &Path) -> bool {
    dir.join("package.json").is_file() || dir.join("wrangler.toml").is_file() || dir.join("wrangler.jsonc").is_file() || dir.join("wrangler.json").is_file() || dir.join("index.html").is_file()
}

/// 检测站点配置。根目录不像站点、而某个常见子目录像时，按那个子目录检测，并记下 build.root。
pub fn detect(dir: &Path) -> Detected {
    if !looks_like_site(dir) {
        if let Some(sub) = SITE_SUBDIRS.iter().find(|s| looks_like_site(&dir.join(s))) {
            let mut d = detect_at(&dir.join(sub));
            d.build.root = (*sub).to_string();
            d.notes.push(format!("站点在仓库的 {sub}/ 子目录里：安装、构建、检查和部署都在那里执行"));
            return d;
        }
    }
    detect_at(dir)
}

fn detect_at(dir: &Path) -> Detected {
    let mut d = Detected::default();
    let pkg = read_json(&dir.join("package.json"));
    let pilot = read_json(&dir.join("pilot.json"));
    let scripts = pkg
        .as_ref()
        .and_then(|p| p.get("scripts"))
        .and_then(|s| s.as_object())
        .cloned()
        .unwrap_or_default();
    let script = |k: &str| scripts.get(k).and_then(|v| v.as_str()).map(str::to_string);

    // 框架与默认产物目录
    let (framework, default_out) = match &pkg {
        Some(p) if has_dep(p, "astro") => ("astro", "dist"),
        Some(p) if has_dep(p, "next") => ("next", "out"),
        Some(p) if has_dep(p, "nuxt") => ("nuxt", ".output/public"),
        Some(p) if has_dep(p, "@sveltejs/kit") => ("sveltekit", "build"),
        Some(p) if has_dep(p, "gatsby") => ("gatsby", "public"),
        Some(p) if has_dep(p, "@11ty/eleventy") => ("eleventy", "_site"),
        Some(p) if has_dep(p, "vitepress") => ("vitepress", "docs/.vitepress/dist"),
        Some(p) if has_dep(p, "vite") => ("vite", "dist"),
        Some(_) => ("node", "dist"),
        None if dir.join("go.mod").exists() => ("go", "."),
        None if dir.join("hugo.toml").exists() || dir.join("config.toml").exists() && dir.join("content").is_dir() => ("hugo", "public"),
        None if dir.join("_config.yml").exists() => ("jekyll", "_site"),
        None => ("static", "."),
    };
    d.framework = framework.into();

    // 构建命令
    if pkg.is_some() {
        d.build.install = install_cmd(dir);
        if script("build").is_some() {
            d.build.build = run_script(dir, "build");
        }
    } else if framework == "hugo" {
        d.build.build = "hugo --minify".into();
    } else if framework == "jekyll" {
        d.build.build = "bundle exec jekyll build".into();
    }
    d.build.out_dir = default_out.into();

    // deploy 脚本里的 wrangler 写法最可信：产物目录、项目名、分支、构建变量都在里面。
    let wrangler = wrangler_config(dir);
    if let Some((name, true, _)) = &wrangler {
        d.deploy_kind = "workers".into();
        d.cf_project = name.clone();
    }
    if let Some(dep) = script("deploy") {
        if dep.contains("wrangler deploy") && !dep.contains("pages deploy") {
            d.deploy_kind = "workers".into();
        }
        let (env, out, project, branch) = parse_deploy_script(&dep);
        if !out.is_empty() && check_out_dir(&out).is_ok() {
            d.build.out_dir = out.trim_start_matches("./").to_string();
        }
        if !project.is_empty() {
            d.cf_project = project;
            d.deploy_kind = "pages".into();
        }
        d.branch = branch;
        if !env.is_empty() {
            d.notes.push(format!(
                "deploy 脚本带了构建变量 {}，已记为站点构建变量",
                env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join("、")
            ));
        }
        d.env = env;
    }
    // CF 建站会话写的 pilot.json：{ build, out }
    if let Some(p) = &pilot {
        if let Some(b) = p.get("build").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) {
            d.build.build = b.trim().into();
        }
        if let Some(o) = p.get("out").and_then(|v| v.as_str()).filter(|s| check_out_dir(s).is_ok()) {
            d.build.out_dir = o.trim().trim_start_matches("./").to_string();
        }
    }
    if d.build.out_dir.is_empty() {
        d.build.out_dir = ".".into();
    }

    // 检查命令：check 与 check:* 脚本
    let mut checks: Vec<&String> = scripts
        .keys()
        .filter(|k| *k == "check" || k.starts_with("check:"))
        .collect();
    checks.sort();
    d.build.checks = checks.into_iter().map(|k| run_script(dir, k)).collect();

    d.build.env_keys = d.env.iter().map(|(k, _)| k.clone()).collect();
    d.domain = domain_from_configs(dir)
        .or_else(|| domain_from_files(dir))
        .or_else(|| wrangler.as_ref().and_then(|(_, _, hosts)| hosts.first().cloned()))
        .unwrap_or_default();
    // VPS：项目自带发布脚本（scp / rsync 到自己的服务器）。Makefile 里的 check / test 目标当作检查。
    if d.deploy_kind.is_empty() {
        for s in ["scripts/publish-production.sh", "scripts/publish.sh", "scripts/deploy.sh", "deploy.sh"] {
            if dir.join(s).is_file() {
                d.deploy_kind = "vps".into();
                d.deploy_command = if s.ends_with("publish-production.sh") { format!("./{s} publish") } else { format!("./{s}") };
                let mut text = fs::read_to_string(dir.join(s)).unwrap_or_default();
                for extra in ["deploy/publish.env", "deploy/publish.env.example"] {
                    if let Ok(t) = fs::read_to_string(dir.join(extra)) {
                        text = format!("{t}\n{text}");
                    }
                }
                d.deploy_target = find_ssh_target(&text).unwrap_or_default();
                break;
            }
        }
    }
    if let Ok(mk) = fs::read_to_string(dir.join("Makefile")) {
        let has = |t: &str| mk.lines().any(|l| l.starts_with(&format!("{t}:")));
        if d.build.checks.is_empty() {
            for t in ["check", "test"] {
                if has(t) {
                    d.build.checks.push(format!("make {t}"));
                }
            }
        }
    }
    if d.domain.is_empty() {
        // .env.example 里的 BASE_URL / SITE_URL
        if let Ok(t) = fs::read_to_string(dir.join(".env.example")) {
            for l in t.lines() {
                let l = l.trim();
                if let Some(v) = l.strip_prefix("BASE_URL=").or_else(|| l.strip_prefix("SITE_URL=")) {
                    if let Ok(h) = normalize_domain(v.trim_matches(['"', '\''])) {
                        if !h.is_empty() && h != "localhost" {
                            d.domain = h;
                            break;
                        }
                    }
                }
            }
        }
    }
    if d.deploy_kind.is_empty() && (pilot.is_some() || !d.cf_project.is_empty()) {
        d.deploy_kind = "pages".into();
    }
    d.indexnow_key = find_indexnow_key(dir, &d.build.out_dir).unwrap_or_default();
    d.has_dotenv = dir.join(".env").exists();
    d.dotenv_example_keys = dotenv_keys(&dir.join(".env.example"));
    if d.cf_project.is_empty() && pilot.is_some() {
        // CF 建站项目：目录名就是 Pages 项目名（cf_system_prompt 约定）。
        if let Some(n) = dir.file_name().and_then(|n| n.to_str()) {
            d.cf_project = n.to_string();
        }
    }

    // git
    if git_out(dir, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true") {
        d.git = true;
        d.git_branch = git_out(dir, &["branch", "--show-current"]).unwrap_or_default();
        d.git_default_branch = git_out(dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
            .map(|s| s.trim_start_matches("origin/").to_string())
            .unwrap_or_else(|| d.git_branch.clone());
        if let Some(url) = git_out(dir, &["remote", "get-url", "origin"]) {
            d.github_repo = github_repo_from_url(&url).unwrap_or_default();
        }
    }
    if d.branch.is_empty() {
        d.branch = if d.git_default_branch.is_empty() { "main".into() } else { d.git_default_branch.clone() };
    }
    d
}

/// 站点名称默认取域名，其次项目名，再次目录名。
pub fn default_name(detected: &Detected, dir: &Path) -> String {
    if !detected.domain.is_empty() {
        return detected.domain.clone();
    }
    if !detected.cf_project.is_empty() {
        return detected.cf_project.clone();
    }
    dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("站点")
        .to_string()
}

// ---------------------------------------------------------------------------
// 构建时变量：值只存钥匙串（一个 JSON 对象），站点元数据里只有名字。
// ---------------------------------------------------------------------------

fn env_account(site_id: &str) -> String {
    format!("site-env:{site_id}")
}

pub fn load_env(site_id: &str) -> Vec<(String, String)> {
    let Ok(raw) = crate::keychain::get_named_secret(&env_account(site_id)) else {
        return Vec::new();
    };
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
    map.into_iter()
        .filter_map(|(k, v)| v.as_str().map(|v| (k, v.to_string())))
        .collect()
}

/// 合并写入：`set` 里的键覆盖，`remove` 里的键删除。返回最终的键名（排序）。
pub fn save_env(site_id: &str, set: &[(String, String)], remove: &[String]) -> Result<Vec<String>, String> {
    let mut map: std::collections::BTreeMap<String, String> = load_env(site_id).into_iter().collect();
    for (k, v) in set {
        check_env_key(k)?;
        if v.contains('\n') {
            return Err(format!("变量 {k} 的值只能写一行"));
        }
        map.insert(k.clone(), v.clone());
    }
    for k in remove {
        map.remove(k);
    }
    if map.is_empty() {
        let _ = crate::keychain::delete_named_secret(&env_account(site_id));
    } else {
        let raw = serde_json::to_string(&map).map_err(|e| e.to_string())?;
        crate::keychain::set_named_secret(&env_account(site_id), &raw)?;
    }
    Ok(map.into_keys().collect())
}

pub fn delete_env(site_id: &str) {
    let _ = crate::keychain::delete_named_secret(&env_account(site_id));
}

#[cfg(test)]
mod tests {


    /// 原子写：写完文件完整，旁边不留临时文件。
    #[test]
    fn write_atomic_leaves_no_temp_file() {
        let d = std::env::temp_dir().join(format!("sites-w-{}", uuid::Uuid::new_v4()));
        let p = d.join("sub").join("vps.json");
        super::write_json(&p, &serde_json::json!({"a": 1}), true).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap().trim(), "{\n  \"a\": 1\n}");
        assert!(!d.join("sub").join("vps.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn gsc_conn_falls_back_to_the_ga4_connection() {
        let mut d = DataBinding { google_conn_id: "a".into(), ..Default::default() };
        assert_eq!(d.gsc_conn(), "a");
        d.gsc_conn_id = "b".into();
        assert_eq!(d.gsc_conn(), "b");
    }

    use super::*;

    #[test]
    fn deploy_script_yields_env_out_project_and_branch() {
        let s = "GA_MEASUREMENT_ID=G-61466WCZ4C npm run build && wrangler pages deploy dist --project-name gatesor --branch main --commit-dirty=true";
        let (env, out, project, branch) = parse_deploy_script(s);
        assert_eq!(env, vec![("GA_MEASUREMENT_ID".into(), "G-61466WCZ4C".into())]);
        assert_eq!(out, "dist");
        assert_eq!(project, "gatesor");
        assert_eq!(branch, "main");
        let (env, out, project, branch) = parse_deploy_script("npx wrangler pages deploy ./build --project-name=x");
        assert!(env.is_empty());
        assert_eq!((out.as_str(), project.as_str(), branch.as_str()), ("./build", "x", ""));
        let (env, out, ..) = parse_deploy_script("vercel deploy");
        assert!(env.is_empty() && out.is_empty());
    }

    #[test]
    fn domains_are_normalized_and_validated() {
        assert_eq!(normalize_domain("https://Gatesor.com/zh/").unwrap(), "gatesor.com");
        assert_eq!(normalize_domain("  www.example.co.uk ").unwrap(), "www.example.co.uk");
        assert_eq!(normalize_domain("").unwrap(), "");
        for bad in ["localhost", "a..b", "-a.com", "a_b.com", "exa mple.com"] {
            assert!(normalize_domain(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn out_dir_must_stay_inside_the_site() {
        assert!(check_out_dir("dist").is_ok());
        assert!(check_out_dir(".").is_ok());
        assert!(check_out_dir("../x").is_err());
        assert!(check_out_dir("/tmp").is_err());
        assert!(check_out_dir("").is_err());
        assert!(check_command("npm run build").is_ok());
        assert!(check_command("npm run build\nrm -rf /").is_err());
    }

    #[test]
    fn github_remotes_parse() {
        assert_eq!(github_repo_from_url("https://github.com/ccvar/gatesor.git").as_deref(), Some("ccvar/gatesor"));
        assert_eq!(github_repo_from_url("git@github.com:ccvar/gatesor.git").as_deref(), Some("ccvar/gatesor"));
        assert_eq!(github_repo_from_url("https://gitlab.com/a/b.git"), None);
    }

    #[test]
    fn detects_an_astro_site_like_gatesor() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        fs::write(
            d.join("package.json"),
            r#"{"scripts":{"build":"astro build","check":"astro check","check:links":"node x.mjs",
            "deploy":"GA_MEASUREMENT_ID=G-1 npm run build && wrangler pages deploy dist --project-name gatesor --branch main"},
            "dependencies":{"astro":"^7"}}"#,
        )
        .unwrap();
        fs::write(d.join("package-lock.json"), "{}").unwrap();
        fs::write(d.join("astro.config.mjs"), "export default defineConfig({\n  site: 'https://gatesor.com',\n});").unwrap();
        fs::create_dir_all(d.join("public")).unwrap();
        fs::write(d.join("public/8c862b9da94589a467d61dad4f75a88f.txt"), "8c862b9da94589a467d61dad4f75a88f\n").unwrap();
        fs::write(d.join(".env.example"), "# x\nINVITE_CODE=\nexport OTHER=1\n").unwrap();
        let det = detect(d);
        assert_eq!(det.framework, "astro");
        assert_eq!(det.build.install, "npm ci");
        assert_eq!(det.build.build, "npm run build");
        assert_eq!(det.build.out_dir, "dist");
        assert_eq!(det.build.checks, vec!["npm run check".to_string(), "npm run check:links".to_string()]);
        assert_eq!(det.build.env_keys, vec!["GA_MEASUREMENT_ID".to_string()]);
        assert_eq!(det.cf_project, "gatesor");
        assert_eq!(det.branch, "main");
        assert_eq!(det.domain, "gatesor.com");
        assert_eq!(det.indexnow_key, "8c862b9da94589a467d61dad4f75a88f");
        assert_eq!(det.dotenv_example_keys, vec!["INVITE_CODE".to_string(), "OTHER".to_string()]);
        assert_eq!(default_name(&det, d), "gatesor.com");
    }

    #[test]
    fn a_site_in_a_subfolder_of_the_repo_is_detected_with_its_root() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        fs::create_dir_all(d.join("site")).unwrap();
        fs::write(d.join("README.md"), "x").unwrap();
        fs::write(d.join("site/package.json"), r#"{"scripts":{"build":"astro build"},"dependencies":{"astro":"^7"}}"#).unwrap();
        fs::write(d.join("site/wrangler.jsonc"), r#"{"name":"cexask","assets":{"directory":"./dist"}}"#).unwrap();
        let det = detect(d);
        assert_eq!(det.build.root, "site");
        assert_eq!(det.framework, "astro");
        assert_eq!(det.deploy_kind, "workers");
        assert!(check_root("site").is_ok() && check_root("../x").is_err() && check_root("/abs").is_err());
    }

    #[test]
    fn detects_workers_projects_from_wrangler_config() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        fs::write(d.join("package.json"), r#"{"scripts":{"build":"astro build","deploy":"npm run build && wrangler deploy && node scripts/indexnow.mjs"},"dependencies":{"astro":"^7"}}"#).unwrap();
        fs::write(
            d.join("wrangler.jsonc"),
            "{\n  // Worker + 静态资源\n  \"$schema\": \"node_modules/wrangler/config-schema.json\",\n  \"name\": \"gategas\",\n  \"main\": \"worker/index.ts\",\n  \"assets\": { \"directory\": \"./dist\", \"run_worker_first\": [\"/*\", \"!/_astro/*\"] },\n}\n",
        )
        .unwrap();
        let det = detect(d);
        assert_eq!(det.deploy_kind, "workers");
        assert_eq!(det.cf_project, "gategas");
        assert_eq!(det.build.out_dir, "dist");

        let t2 = tempfile::tempdir().unwrap();
        let d2 = t2.path();
        fs::write(d2.join("index.html"), "x").unwrap();
        fs::write(
            d2.join("wrangler.toml"),
            "# Worker\nname = \"gateraw\"\nmain = \"worker/index.js\"\nroutes = [{ pattern = \"gateraw.com/*\", zone_name = \"gateraw.com\" }]\n\n[assets]\ndirectory = \"./dist\"\n",
        )
        .unwrap();
        let (name, workers, hosts) = wrangler_config(d2).unwrap();
        assert_eq!((name.as_str(), workers), ("gateraw", true));
        assert_eq!(hosts, vec!["gateraw.com".to_string()]);
        assert_eq!(detect(d2).domain, "gateraw.com");

        let t3 = tempfile::tempdir().unwrap();
        fs::write(t3.path().join("wrangler.toml"), "name = \"site\"\npages_build_output_dir = \"dist\"\n").unwrap();
        assert!(!wrangler_config(t3.path()).unwrap().1);
    }

    #[test]
    fn ssh_targets_are_found_in_publish_scripts() {
        assert_eq!(find_ssh_target("target=${OANEO_SSH_TARGET:-root@74.82.198.105}").as_deref(), Some("root@74.82.198.105"));
        assert_eq!(find_ssh_target("OANEO_SSH_TARGET='deploy@srv.example.org'\nX=1").as_deref(), None);
        assert_eq!(find_ssh_target("rsync -az dist/ web@my-box.net:/srv/www").as_deref(), Some("web@my-box.net"));
        assert_eq!(find_ssh_target("echo hi"), None);
    }

    #[test]
    fn detects_a_plain_static_cf_project() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("my-site");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("index.html"), "<h1>hi</h1>").unwrap();
        fs::write(d.join("pilot.json"), r#"{"dev":"wrangler pages dev .","port":8788}"#).unwrap();
        let det = detect(&d);
        assert_eq!(det.framework, "static");
        assert_eq!(det.build.build, "");
        assert_eq!(det.build.install, "");
        assert_eq!(det.build.out_dir, ".");
        assert_eq!(det.cf_project, "my-site");
    }

    #[test]
    fn store_rejects_duplicates_and_keeps_identity_on_update() {
        let t = tempfile::tempdir().unwrap();
        let store = SiteStore::new(t.path());
        let site = Site {
            id: String::new(),
            name: "a".into(),
            domain: String::new(),
            source: "local".into(),
            gh_conn_id: String::new(),
            repo: String::new(),
            local_dir: "/x".into(),
            default_branch: "main".into(),
            framework: String::new(),
            build: BuildConfig::default(),
            deploy: DeployConfig::default(),
            data: DataBinding::default(),
            ops: OpsConfig::default(),
            pinned: false,
            tags: Vec::new(),
            origin: String::new(),
            migrate_from: String::new(),
            created_at: 1,
            updated_at: 1,
        };
        let saved = store.insert(site.clone()).unwrap();
        assert!(store.insert(site).is_err());
        let up = store
            .update(&saved.id, 9, |s| {
                s.id = "hijack".into();
                s.name = "b".into();
                Ok(())
            })
            .unwrap();
        assert_eq!(up.id, saved.id);
        assert_eq!(up.name, "b");
        assert_eq!(up.created_at, 1);
        assert_eq!(up.updated_at, 9);
        assert!(store.data_dir(&saved.id).is_dir());
        store.remove(&saved.id).unwrap();
        assert!(!store.data_dir(&saved.id).exists());
    }

    #[test]
    fn ops_config_defaults_to_self_run_level_one_with_an_empty_whitelist() {
        let c: OpsConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(c.level, 1);
        assert!(c.whitelist.is_empty());
        assert_eq!(c.budget.changes_per_week, 10);
        assert_eq!(c.brain, "claude");
    }
}
