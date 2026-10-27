//! 站点配置变量：邀请链接、统计 ID、联系邮箱、站点地址、第三方 API Key 这类「运营配置」不写死在代码里。
//!
//! - 声明：仓库里的 `.env.example`（变量名、用途、[public] / [secret]），由 AI 制作站点时写；
//! - 值：存在 Pilot（钥匙串 `site-env:<站点 id>`），Pilot 构建时注入；AI 只知道变量名，看不到值；
//! - 同步：按部署方式写到对应的地方——Pages 项目的环境变量 / Worker 的密钥（公开变量部署时用 --var 带上）/
//!   服务器上的 `/opt/<站点>/config/<站点>.env`（合并写，不动 Pilot 没管的行）。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::conns::ConnStore;
use crate::sites::{self, Site};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ConfigVar {
    pub key: String,
    pub desc: String,
    pub secret: bool,
    /// 在 .env.example 里声明过（没声明的是手动加的变量）
    pub declared: bool,
    /// Pilot 里已经有值
    pub set: bool,
    /// 没填值时 Pilot 自动给的值（比如 SITE_URL＝https://<站点域名>）；填了就用填的
    pub auto: String,
    /// Pilot 里没有值，但从线上读到它已经设过（Cloudflare 的密钥只给名字不给值）
    #[serde(default)]
    pub remote: bool,
}

/// 解析 `.env.example`：变量上方的注释是说明，`[secret]` / `[public]` 标明类别；没标的按名字猜。
pub fn parse_example(text: &str) -> Vec<(String, String, bool)> {
    let mut out = vec![];
    let mut note: Vec<String> = vec![];
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            note.clear();
            continue;
        }
        if let Some(c) = l.strip_prefix('#') {
            note.push(c.trim().to_string());
            continue;
        }
        let l = l.strip_prefix("export ").unwrap_or(l);
        let Some((k, _)) = l.split_once('=') else { continue };
        let k = k.trim();
        if sites::check_env_key(k).is_err() {
            note.clear();
            continue;
        }
        let raw = note.join(" ");
        let low = raw.to_lowercase();
        let secret = if low.contains("[secret]") || raw.contains("[密钥]") {
            true
        } else if low.contains("[public]") || raw.contains("[公开]") {
            false
        } else {
            guess_secret(k)
        };
        let desc = raw.replace("[secret]", "").replace("[public]", "").replace("[SECRET]", "").replace("[PUBLIC]", "").replace("[密钥]", "").replace("[公开]", "");
        if !out.iter().any(|(x, _, _): &(String, String, bool)| x == k) {
            out.push((k.to_string(), desc.trim().to_string(), secret));
        }
        note.clear();
    }
    out
}

/// 不用填、Pilot 能按站点信息自动给的变量：站点地址类（SITE_URL / PUBLIC_SITE_URL / SITE）＝https://<站点域名>。
pub fn derived(site: &Site, key: &str) -> Option<String> {
    let u = key.to_ascii_uppercase();
    let site_url = u.ends_with("SITE_URL") || u == "SITE" || u == "PUBLIC_SITE";
    (site_url && !site.domain.trim().is_empty()).then(|| format!("https://{}", site.domain.trim()))
}

/// 构建、同步时实际用的值：填过的 + 声明了、没填、但能自动给的。
pub fn effective_values(site: &Site, code: &Path) -> Vec<(String, String)> {
    let mut v = sites::load_env(&site.id);
    for c in vars(site, code) {
        if c.declared && !c.set && !c.auto.is_empty() && !v.iter().any(|(k, _)| k == &c.key) {
            v.push((c.key, c.auto));
        }
    }
    v
}

/// 按框架约定会打进前端包的前缀：这种变量不可能是密钥。
fn public_prefix(u: &str) -> bool {
    ["PUBLIC_", "NEXT_PUBLIC_", "VITE_", "NUXT_PUBLIC_", "REACT_APP_", "GATSBY_"].iter().any(|p| u.starts_with(p))
}

/// `.env.example` 里声明了、但没标 [secret] / [public] 的变量：按名字猜。
/// 名字里带这些词的都当密钥（SENTRY_DSN、SMTP_PASS 这类凭据也算）；带 URL / URI 的只有像连接串的才算
/// （DATABASE_URL、REDIS_URL、WEBHOOK_URL……），BASE_URL、SITE_URL 这类站点地址是公开的。
fn guess_secret(k: &str) -> bool {
    let u = k.to_ascii_uppercase();
    if public_prefix(&u) {
        return false;
    }
    let url = u.contains("URL") || u.contains("URI");
    (url && CONN_WORDS.iter().any(|w| u.contains(w))) || SECRET_WORDS.iter().any(|w| u.contains(w))
}

const SECRET_WORDS: [&str; 14] = ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "PASS", "PRIVATE", "DSN", "CRED", "AUTH", "CERT", "SIGNING", "SALT", "HASH"];
/// 名字里有 URL 时，再带这些词才像带凭据的连接串
const CONN_WORDS: [&str; 12] = ["DATABASE", "DB_", "_DB", "REDIS", "MONGO", "POSTGRES", "MYSQL", "AMQP", "RABBIT", "HOOK", "SMTP", "CONN"];

/// 没在 `.env.example` 里声明、在 Pilot 里手动加的变量：一律按密钥处理（不进命令行、Pages 里存成 secret_text），
/// 只有框架约定会打进前端包的前缀（PUBLIC_ 等）例外——那种值本来就在页面源码里。
fn undeclared_secret(k: &str) -> bool {
    !public_prefix(&k.to_ascii_uppercase())
}

/// `.env.example` 在哪：站点子目录优先，其次仓库根目录。
pub fn example_path(site: &Site, code: &Path) -> Option<PathBuf> {
    [site.app_dir(code).join(".env.example"), code.join(".env.example")].into_iter().find(|p| p.is_file())
}

/// 全部配置变量：.env.example 里声明的 + Pilot 里手动加过的。
pub fn vars(site: &Site, code: &Path) -> Vec<ConfigVar> {
    let declared = example_path(site, code).and_then(|p| std::fs::read_to_string(p).ok()).map(|t| parse_example(&t)).unwrap_or_default();
    let mut out: Vec<ConfigVar> = declared
        .into_iter()
        .map(|(key, desc, secret)| ConfigVar { set: site.build.env_keys.contains(&key), auto: derived(site, &key).unwrap_or_default(), key, desc, secret, declared: true, remote: false })
        .collect();
    for k in &site.build.env_keys {
        if !out.iter().any(|v| &v.key == k) {
            out.push(ConfigVar { key: k.clone(), desc: String::new(), secret: undeclared_secret(k), declared: false, set: true, auto: String::new(), remote: false });
        }
    }
    out
}

/// 变量名 → 是不是密钥（声明的按声明 / 名字猜，没声明的按 `undeclared_secret`）。查不到的一律当密钥。
pub fn secret_map(site: &Site, code: &Path) -> std::collections::HashMap<String, bool> {
    vars(site, code).into_iter().map(|v| (v.key, v.secret)).collect()
}

/// .env.example 里声明的 GA4 衡量 ID 变量（优先 PUBLIC_GA_ID，其次名字像 GA_MEASUREMENT_ID / GA4_ID 的）。
pub fn ga_key(vars: &[ConfigVar]) -> Option<String> {
    let declared: Vec<&ConfigVar> = vars.iter().filter(|v| v.declared && !v.secret).collect();
    if let Some(v) = declared.iter().find(|v| v.key == "PUBLIC_GA_ID") {
        return Some(v.key.clone());
    }
    declared
        .iter()
        .find(|v| {
            let u = v.key.to_ascii_uppercase();
            let ga = u.contains("GA4") || u.starts_with("GA_") || u.contains("_GA_") || u.ends_with("_GA") || u.contains("GTAG");
            ga && (u.contains("ID") || u.contains("MEASUREMENT"))
        })
        .map(|v| v.key.clone())
}

/// 同步到哪：pages | workers | vps；暂不部署时为空（只在 Pilot 构建时用）。
pub fn target_of(site: &Site) -> &'static str {
    match site.deploy.mode.as_str() {
        "direct" | "pages_git" => "pages",
        "workers" => "workers",
        "vps" => "vps",
        _ => "",
    }
}

/// 服务器上的配置文件：没填就是 /opt/<站点>/config/<站点>.env（站点＝域名的第一段，或站点名的 slug）。
pub fn env_file(site: &Site) -> String {
    if !site.deploy.env_file.trim().is_empty() {
        return site.deploy.env_file.trim().to_string();
    }
    let base = site.domain.split('.').next().filter(|s| !s.is_empty()).map(|s| s.to_string()).unwrap_or_else(|| crate::site_git::slugify(&site.name, 30));
    format!("/opt/{base}/config/{base}.env")
}

pub fn target_label(site: &Site) -> String {
    match target_of(site) {
        "pages" => format!("Cloudflare Pages 项目「{}」的环境变量", site.deploy.project),
        "workers" => format!("Worker「{}」的变量和密钥", site.deploy.project),
        "vps" => format!("服务器上的 {}", env_file(site)),
        _ => "只在 Pilot 构建时使用（还没配置部署）".into(),
    }
}

/// 上次同步了什么（存在 Pilot 的站点数据目录）：用来判断「有改动没同步」，以及删掉的变量要不要从目标那边删。
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SyncRecord {
    pub target: String,
    pub hash: String,
    pub keys: Vec<String>,
    pub at: u64,
    /// 上次「从线上读取」时线上已经设了、但读不到值的变量（Cloudflare 的密钥）
    #[serde(default)]
    pub remote: Vec<String>,
}

pub fn load_sync(site_dir: &Path) -> SyncRecord {
    std::fs::read(site_dir.join("config_sync.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_sync(site_dir: &Path, r: &SyncRecord) -> Result<(), String> {
    sites::write_json(&site_dir.join("config_sync.json"), r, false)
}

/// 值的指纹（不存值本身）：目标 + 排好序的 键=值。
pub fn values_hash(site: &Site, values: &[(String, String)]) -> String {
    let mut v = values.to_vec();
    v.sort();
    let mut h = Sha256::new();
    h.update(format!("{}|{}|{}|", target_of(site), site.deploy.project, env_file(site)));
    for (k, val) in v {
        h.update(format!("{k}={val}\n"));
    }
    format!("{:x}", h.finalize())
}

#[derive(Serialize, Clone)]
pub struct ConfigState {
    pub vars: Vec<ConfigVar>,
    /// 声明了但还没填值的
    pub missing: Vec<String>,
    pub target: String,
    pub target_label: String,
    /// 有值、有同步目标，而且和上次同步的不一样
    pub needs_sync: bool,
    pub synced_at: u64,
    pub env_file: String,
}

pub fn state(site: &Site, code: &Path, site_dir: &Path) -> ConfigState {
    let target = target_of(site);
    let rec = load_sync(site_dir);
    let mut vars = vars(site, code);
    if rec.target == target {
        for v in vars.iter_mut().filter(|v| !v.set) {
            v.remote = rec.remote.contains(&v.key);
        }
    }
    let missing = vars.iter().filter(|v| v.declared && !v.set && v.auto.is_empty() && !v.remote).map(|v| v.key.clone()).collect();
    let values = effective_values(site, code);
    let changed = rec.target != target || rec.hash != values_hash(site, &values);
    // 删光了但以前同步过：也要同步一次（把那边的删掉）
    let needs_sync = !target.is_empty() && (!values.is_empty() || !rec.keys.is_empty()) && changed;
    ConfigState { vars, missing, target: target.into(), target_label: target_label(site), needs_sync, synced_at: rec.at, env_file: if target == "vps" { env_file(site) } else { String::new() } }
}

/// 合并写服务器上的 env 文件：Pilot 管的键替换或追加，删掉的键去掉，其它行原样保留。
pub fn merge_env_file(existing: &str, set: &[(String, String)], remove: &[String]) -> String {
    let mut out: Vec<String> = vec![];
    let mut done: Vec<&str> = vec![];
    for line in existing.lines() {
        let t = line.trim_start();
        let t = t.strip_prefix("export ").unwrap_or(t);
        let key = t.split_once('=').map(|(k, _)| k.trim()).filter(|_| !t.starts_with('#'));
        match key {
            Some(k) if remove.iter().any(|r| r == k) => {}
            Some(k) => match set.iter().find(|(s, _)| s == k) {
                Some((s, v)) => {
                    out.push(format!("{s}={v}"));
                    done.push(s);
                }
                None => out.push(line.to_string()),
            },
            None => out.push(line.to_string()),
        }
    }
    let rest: Vec<&(String, String)> = set.iter().filter(|(k, _)| !done.contains(&k.as_str())).collect();
    if !rest.is_empty() {
        if !out.is_empty() && !out.last().map(|l| l.trim().is_empty()).unwrap_or(true) {
            out.push(String::new());
        }
        out.push("# 以下由 Cligc Pilot 写入（站点设置 → 配置变量）".into());
        for (k, v) in rest {
            out.push(format!("{k}={v}"));
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// 同步到部署目标。`code`：站点代码目录（读 .env.example 判断哪些是密钥）。返回一句给人看的结果。
pub async fn sync(site: &Site, conns: &ConnStore, code: &Path, site_dir: &Path) -> Result<String, String> {
    let target = target_of(site);
    if target.is_empty() {
        return Err("还没配置部署：配置变量只在 Pilot 构建时使用".into());
    }
    let values = effective_values(site, code);
    let rec = load_sync(site_dir);
    let remove: Vec<String> = if rec.target == target { rec.keys.iter().filter(|k| !values.iter().any(|(v, _)| v == *k)).cloned().collect() } else { vec![] };
    let secret_of = secret_map(site, code);
    let is_secret = |k: &str| secret_of.get(k).copied().unwrap_or(true);
    let note = match target {
        "pages" | "workers" => {
            let cf = conns.get(&site.deploy.cf_conn_id).map_err(|_| "请先在站点设置里选择 Cloudflare 连接".to_string())?;
            let token = crate::keychain::get_key(&cf.id)?;
            if site.deploy.project.trim().is_empty() {
                return Err("请先在站点设置里填写 Pages 项目 / Worker 名称".into());
            }
            if target == "pages" {
                if crate::cf::get_pages_project(&token, &cf.account_id, &site.deploy.project).await?.is_none() {
                    return Err(format!("Pages 项目「{}」还没建：先在上线向导里创建，或上线一次", site.deploy.project));
                }
                let vars: Vec<(String, String, bool)> = values.iter().map(|(k, v)| (k.clone(), v.clone(), is_secret(k))).collect();
                crate::cf::set_pages_env(&token, &cf.account_id, &site.deploy.project, &vars, &remove).await?;
                format!("已写入 Pages 项目「{}」的环境变量（{} 个，生产和预览）", site.deploy.project, values.len())
            } else {
                let mut n = 0;
                for (k, v) in values.iter().filter(|(k, _)| is_secret(k)) {
                    crate::cf::put_worker_secret(&token, &cf.account_id, &site.deploy.project, k, v)
                        .await
                        .map_err(|e| if e.contains("404") || e.contains("10007") { format!("Worker「{}」还没上线过：先上线一次再同步密钥", site.deploy.project) } else { e })?;
                    n += 1;
                }
                for k in remove.iter().filter(|k| is_secret(k)) {
                    crate::cf::delete_worker_secret(&token, &cf.account_id, &site.deploy.project, k).await?;
                }
                format!("已写入 Worker「{}」的 {n} 个密钥；公开变量在每次上线时随部署带上", site.deploy.project)
            }
        }
        "vps" => {
            let ssh = conns.get(&site.deploy.ssh_conn_id).map_err(|_| "请先在站点设置里选择部署用的 SSH 连接".to_string())?;
            write_server_env(&ssh, &env_file(site), &values, &remove).await?;
            format!("已写入服务器上的 {}（{} 个变量，其它行原样保留）", env_file(site), values.len())
        }
        _ => unreachable!(),
    };
    save_sync(site_dir, &SyncRecord { target: target.into(), hash: values_hash(site, &values), keys: values.iter().map(|(k, _)| k.clone()).collect(), at: crate::now_secs(), remote: rec.remote.iter().filter(|k| !remove.contains(k) && !values.iter().any(|(v, _)| v == *k)).cloned().collect() })?;
    Ok(note)
}

/// 解析 env 文件：`KEY=VALUE` 一行一个；跳过注释和空行，去掉 `export ` 和成对的引号。
/// 行尾的 `# …` 不当注释（Pilot 写的值不加引号，docker / systemd 的 env 文件也不认行内注释）。
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        let t = t.strip_prefix("export ").unwrap_or(t).trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let Some((k, v)) = t.split_once('=') else { continue };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let v = v.trim();
        let v = if v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))) {
            v[1..v.len() - 1].to_string()
        } else {
            v.to_string()
        };
        out.push((k.to_string(), v));
    }
    out
}

/// 从线上读回来的结果（只对 .env.example 里声明的、或 Pilot 已经管着的变量）。
#[derive(Default, Debug)]
pub struct Pulled {
    /// Pilot 里没有、线上有值：填进 Pilot
    pub fill: Vec<(String, String)>,
    /// 两边一样
    pub same: usize,
    /// 两边都有、但不一样（保留 Pilot 的，下次同步会推上去）
    pub differ: Vec<String>,
    /// 线上设了、但读不到值（Cloudflare 的密钥）
    pub hidden: Vec<String>,
    /// 线上有、但没在 .env.example 里声明的（不导入，线上照常保留）
    pub extra: Vec<String>,
    /// Pilot 里有、线上没有的
    pub local_only: Vec<String>,
    /// 两边都有，但线上的值读不到、比不了（Cloudflare 的密钥）
    pub unknown: usize,
}

impl Pulled {
    /// 读完以后两边确定一致：「同步」按钮不用再提醒
    pub fn in_sync(&self) -> bool {
        self.differ.is_empty() && self.local_only.is_empty() && self.unknown == 0
    }
}

/// 线上变量（值读不到的是 None）和 Pilot 里的对一遍。
pub fn compare_remote(remote: &[(String, Option<String>)], known: &[String], local: &[(String, String)]) -> Pulled {
    let mut p = Pulled::default();
    for (k, v) in remote {
        if !known.contains(k) {
            p.extra.push(k.clone());
            continue;
        }
        let mine = local.iter().find(|(lk, _)| lk == k).map(|(_, lv)| lv);
        match (v, mine) {
            (None, None) => p.hidden.push(k.clone()),
            (None, Some(_)) => p.unknown += 1,
            (Some(v), None) if !v.is_empty() => p.fill.push((k.clone(), v.clone())),
            (Some(_), None) => {}
            (Some(v), Some(m)) if v == m => p.same += 1,
            (Some(_), Some(_)) => p.differ.push(k.clone()),
        }
    }
    p.local_only = local.iter().filter(|(k, _)| !remote.iter().any(|(r, _)| r == k)).map(|(k, _)| k.clone()).collect();
    p
}

/// 从线上读取：读线上现在的变量，和 Pilot 里的对一遍。值由调用方存进钥匙串，再调 `record_pull`。
pub async fn pull(site: &Site, conns: &ConnStore, code: &Path) -> Result<Pulled, String> {
    let remote = read_remote(site, conns).await?;
    let known: Vec<String> = vars(site, code).into_iter().map(|v| v.key).collect();
    Ok(compare_remote(&remote, &known, &effective_values(site, code)))
}

/// 读取完记一笔：哪些密钥线上有但读不到（不再显示「未填」）；两边一致的话，把「同步」的提醒也消掉。
/// `site`、`code` 要用存完值以后的。返回一句给人看的结果。
pub fn record_pull(site: &Site, code: &Path, site_dir: &Path, p: &Pulled) -> Result<String, String> {
    let target = target_of(site);
    let mut rec = load_sync(site_dir);
    if rec.target != target {
        rec = SyncRecord { target: target.into(), ..Default::default() };
    }
    rec.remote = p.hidden.clone();
    if p.in_sync() {
        let values = effective_values(site, code);
        rec.hash = values_hash(site, &values);
        rec.keys = values.into_iter().map(|(k, _)| k).collect();
        rec.at = crate::now_secs();
    }
    save_sync(site_dir, &rec)?;
    let mut parts = vec![];
    if !p.fill.is_empty() {
        parts.push(format!("读回 {} 个值", p.fill.len()));
    }
    if p.same > 0 {
        parts.push(format!("{} 个和线上一样", p.same));
    }
    if !p.hidden.is_empty() {
        parts.push(format!("{} 个密钥线上已设（Cloudflare 不给看值）", p.hidden.len()));
    }
    if !p.differ.is_empty() {
        parts.push(format!("{} 和线上不一样，保留 Pilot 里的（同步时会写上去）", p.differ.join("、")));
    }
    if !p.extra.is_empty() {
        parts.push(format!("线上另有 {} 个没在 .env.example 里声明的变量，没导入", p.extra.len()));
    }
    Ok(if parts.is_empty() { "线上没有这个站点用得到的变量".into() } else { parts.join("；") })
}

/// 读线上现在的配置变量：服务器读 env 文件；Pages 读项目的生产环境变量；Workers 读脚本设置里的绑定。
pub async fn read_remote(site: &Site, conns: &ConnStore) -> Result<Vec<(String, Option<String>)>, String> {
    match target_of(site) {
        "vps" => {
            let ssh = conns.get(&site.deploy.ssh_conn_id).map_err(|_| "请先在站点设置里选择部署用的 SSH 连接".to_string())?;
            Ok(parse_env_file(&read_server_env(&ssh, &env_file(site)).await?).into_iter().map(|(k, v)| (k, Some(v))).collect())
        }
        t @ ("pages" | "workers") => {
            let cf = conns.get(&site.deploy.cf_conn_id).map_err(|_| "请先在站点设置里选择 Cloudflare 连接".to_string())?;
            let token = crate::keychain::get_key_async(&cf.id).await?;
            if site.deploy.project.trim().is_empty() {
                return Err("请先在站点设置里填写 Pages 项目 / Worker 名称".into());
            }
            if t == "pages" {
                crate::cf::pages_env_vars(&token, &cf.account_id, &site.deploy.project).await
            } else {
                crate::cf::worker_vars(&token, &cf.account_id, &site.deploy.project).await
            }
        }
        _ => Err("还没配置部署：没有线上可以读".into()),
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn check_env_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') || path.contains("..") || path.ends_with('/') {
        return Err(format!("配置文件路径不对：{path}"));
    }
    Ok(())
}

/// 用 Pilot 的部署密钥读服务器上的 env 文件（不存在就是空）。
async fn read_server_env(ssh: &crate::conns::Connection, path: &str) -> Result<String, String> {
    check_env_path(path)?;
    let shim = crate::site_vps::shim(&ssh.id)?;
    let target = format!("{}@{}", ssh.ssh_user, ssh.ssh_host);
    let read = tokio::process::Command::new(shim.bin_dir().join("ssh"))
        .args(["-p", &ssh.ssh_port.to_string(), &target, &format!("cat {} 2>/dev/null || true", shell_quote(path))])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("启动 ssh 失败：{e}"))?;
    if !read.status.success() {
        return Err(format!("读取服务器上的 {path} 失败：{}（先在站点设置里为部署授权）", String::from_utf8_lossy(&read.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&read.stdout).into_owned())
}

/// 用 Pilot 的部署密钥登录服务器：读出现有的 env 文件，合并后原子写回（权限 600）。
async fn write_server_env(ssh: &crate::conns::Connection, path: &str, set: &[(String, String)], remove: &[String]) -> Result<(), String> {
    let existing = read_server_env(ssh, path).await?;
    let shim = crate::site_vps::shim(&ssh.id)?;
    let target = format!("{}@{}", ssh.ssh_user, ssh.ssh_host);
    let ssh_bin = shim.bin_dir().join("ssh");
    let port = ssh.ssh_port.to_string();
    let q = shell_quote(path);
    let merged = merge_env_file(&existing, set, remove);
    let dir = Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into());
    let cmd = format!("mkdir -p {d} && umask 077 && cat > {t} && chmod 600 {t} && mv {t} {q}", d = shell_quote(&dir), t = shell_quote(&format!("{path}.pilot-tmp")));
    let mut child = tokio::process::Command::new(&ssh_bin)
        .args(["-p", &port, &target, &cmd])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 ssh 失败：{e}"))?;
    {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().ok_or("ssh 没有标准输入")?;
        stdin.write_all(merged.as_bytes()).await.map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("写入服务器上的 {path} 失败：{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_file_declares_keys_descriptions_and_secrets() {
        let v = parse_example("# 币安邀请链接 [public]\nPUBLIC_BINANCE_REF_URL=https://example.com\n\n# CoinGecko 接口密钥 [secret]\nCOINGECKO_KEY=\n# 统计 ID\nexport GA_ID=G-XXXX\nAPI_TOKEN=\nnot a var\n");
        assert_eq!(v[0], ("PUBLIC_BINANCE_REF_URL".into(), "币安邀请链接".into(), false));
        assert_eq!(v[1], ("COINGECKO_KEY".into(), "CoinGecko 接口密钥".into(), true));
        assert_eq!(v[2], ("GA_ID".into(), "统计 ID".into(), false));
        assert_eq!(v[3], ("API_TOKEN".into(), "".into(), true), "没标的按名字猜");
        assert_eq!(v.len(), 4);
    }

    #[test]
    fn connection_strings_and_credentials_count_as_secrets() {
        for k in ["DATABASE_URL", "REDIS_URL", "MONGODB_URI", "SLACK_WEBHOOK_URL", "SMTP_PASS", "SENTRY_DSN", "GOOGLE_CREDENTIALS", "AUTH_SECRET", "TLS_CERT", "JWT_SIGNING_KEY", "DB_PASSWD"] {
            assert!(guess_secret(k), "{k}");
        }
        for k in ["SITE_URL", "BASE_URL", "API_BASE_URL", "CDN_URL", "PUBLIC_SITE_URL", "PUBLIC_API_URL", "NEXT_PUBLIC_KEY", "GA_ID", "CONTACT_EMAIL"] {
            assert!(!guess_secret(k), "{k}");
        }
        // 没声明的变量一律当密钥，只有前端前缀例外
        assert!(undeclared_secret("CONTACT_EMAIL") && undeclared_secret("FOO"));
        assert!(!undeclared_secret("PUBLIC_GA_ID") && !undeclared_secret("VITE_TITLE"));
    }

    #[test]
    fn ga_variable_is_found_by_name() {
        let v = |k: &str| ConfigVar { key: k.into(), desc: String::new(), secret: false, declared: true, set: false, auto: String::new(), remote: false };
        assert_eq!(ga_key(&[v("GA_MEASUREMENT_ID"), v("PUBLIC_GA_ID")]).as_deref(), Some("PUBLIC_GA_ID"));
        assert_eq!(ga_key(&[v("PUBLIC_SITE_URL"), v("PUBLIC_GA4_ID")]).as_deref(), Some("PUBLIC_GA4_ID"));
        assert_eq!(ga_key(&[v("GA_MEASUREMENT_ID")]).as_deref(), Some("GA_MEASUREMENT_ID"));
        assert_eq!(ga_key(&[v("PUBLIC_GAME_ID"), v("PUBLIC_SITE_URL")]), None);
    }

    #[test]
    fn server_env_file_is_merged_not_overwritten() {
        let old = "# ngneo\nDB_URL=postgres://x\nREF_URL=old\nexport GA_ID=G-1\nGONE=1\n";
        let merged = merge_env_file(old, &[("REF_URL".into(), "new".into()), ("GA_ID".into(), "G-2".into()), ("NEW_KEY".into(), "v".into())], &["GONE".into()]);
        assert!(merged.contains("DB_URL=postgres://x"), "Pilot 没管的行原样保留");
        assert!(merged.contains("REF_URL=new") && merged.contains("GA_ID=G-2") && !merged.contains("REF_URL=old"));
        assert!(!merged.contains("GONE=1") && merged.contains("NEW_KEY=v") && merged.contains("# ngneo"));
        assert_eq!(merge_env_file("", &[("A".into(), "1".into())], &[]), "# 以下由 Cligc Pilot 写入（站点设置 → 配置变量）\nA=1\n");
    }

    #[test]
    fn default_server_env_path_follows_the_site_name() {
        let mut s: Site = serde_json::from_value(serde_json::json!({"id":"x","name":"NG Neo","domain":"ngneo.com","source":"local","created_at":0,"updated_at":0})).unwrap();
        assert_eq!(env_file(&s), "/opt/ngneo/config/ngneo.env");
        s.domain.clear();
        assert_eq!(env_file(&s), "/opt/ng-neo/config/ng-neo.env");
        s.deploy.env_file = "/srv/a.env".into();
        assert_eq!(env_file(&s), "/srv/a.env");
        assert_eq!(derived(&s, "SITE_URL"), None, "没有域名就不自动给");
        s.domain = "ngneo.com".into();
        assert_eq!(derived(&s, "PUBLIC_SITE_URL").as_deref(), Some("https://ngneo.com"));
        assert_eq!(derived(&s, "SITE").as_deref(), Some("https://ngneo.com"));
        assert_eq!(derived(&s, "PUBLIC_GA_ID"), None);
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn server_env_file_is_read_back() {
        let text = "# 注释\n\nexport A=1\nB=\"two words\"\nC='x=y'\nD=a # b\nbad line\nE=\n";
        assert_eq!(
            parse_env_file(text),
            vec![("A".into(), "1".into()), ("B".into(), "two words".into()), ("C".into(), "x=y".into()), ("D".into(), "a # b".into()), ("E".into(), String::new())]
        );
        // Pilot 自己写进去的，读回来一样
        let written = merge_env_file("", &[("K".into(), "v w".into())], &[]);
        assert_eq!(parse_env_file(&written), vec![("K".to_string(), "v w".to_string())]);
    }

    #[test]
    fn pulling_fills_only_what_pilot_lacks() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let remote = vec![
            ("BASE_URL".to_string(), Some("https://a.com".to_string())),
            ("SITE_URL".to_string(), Some("https://a.com".to_string())),
            ("API_KEY".to_string(), None),
            ("TOKEN".to_string(), Some("new".to_string())),
            ("STRAY".to_string(), Some("1".to_string())),
            ("EMPTY".to_string(), Some(String::new())),
        ];
        let known = s(&["BASE_URL", "SITE_URL", "API_KEY", "TOKEN", "EMPTY", "LOCAL"]);
        let local = vec![("SITE_URL".to_string(), "https://a.com".to_string()), ("TOKEN".to_string(), "old".to_string()), ("LOCAL".to_string(), "x".to_string())];
        let p = compare_remote(&remote, &known, &local);
        assert_eq!(p.fill, vec![("BASE_URL".to_string(), "https://a.com".to_string())]);
        assert_eq!(p.same, 1);
        assert_eq!(p.differ, s(&["TOKEN"]));
        assert_eq!(p.hidden, s(&["API_KEY"]));
        assert_eq!(p.extra, s(&["STRAY"]));
        assert_eq!(p.local_only, s(&["LOCAL"]));
        assert!(!p.in_sync());
        let p = compare_remote(&remote[..2], &known, &local[..1]);
        assert!(p.in_sync());
    }
}
