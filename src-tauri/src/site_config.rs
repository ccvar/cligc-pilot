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

fn guess_secret(k: &str) -> bool {
    let u = k.to_ascii_uppercase();
    !u.starts_with("PUBLIC_") && ["KEY", "TOKEN", "SECRET", "PASSWORD", "PRIVATE"].iter().any(|w| u.contains(w))
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
        .map(|(key, desc, secret)| ConfigVar { set: site.build.env_keys.contains(&key), key, desc, secret, declared: true })
        .collect();
    for k in &site.build.env_keys {
        if !out.iter().any(|v| &v.key == k) {
            out.push(ConfigVar { key: k.clone(), desc: String::new(), secret: guess_secret(k), declared: false, set: true });
        }
    }
    out
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
}

pub fn load_sync(site_dir: &Path) -> SyncRecord {
    std::fs::read(site_dir.join("config_sync.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_sync(site_dir: &Path, r: &SyncRecord) -> Result<(), String> {
    std::fs::create_dir_all(site_dir).map_err(|e| e.to_string())?;
    std::fs::write(site_dir.join("config_sync.json"), serde_json::to_vec(r).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
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
    let vars = vars(site, code);
    let missing = vars.iter().filter(|v| v.declared && !v.set).map(|v| v.key.clone()).collect();
    let target = target_of(site);
    let rec = load_sync(site_dir);
    let values = sites::load_env(&site.id);
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
    let values = sites::load_env(&site.id);
    let rec = load_sync(site_dir);
    let remove: Vec<String> = if rec.target == target { rec.keys.iter().filter(|k| !values.iter().any(|(v, _)| v == *k)).cloned().collect() } else { vec![] };
    let secret_of: std::collections::HashMap<String, bool> = vars(site, code).into_iter().map(|v| (v.key, v.secret)).collect();
    let is_secret = |k: &str| secret_of.get(k).copied().unwrap_or_else(|| guess_secret(k));
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
    save_sync(site_dir, &SyncRecord { target: target.into(), hash: values_hash(site, &values), keys: values.iter().map(|(k, _)| k.clone()).collect(), at: crate::now_secs() })?;
    Ok(note)
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 用 Pilot 的部署密钥登录服务器：读出现有的 env 文件，合并后原子写回（权限 600）。
async fn write_server_env(ssh: &crate::conns::Connection, path: &str, set: &[(String, String)], remove: &[String]) -> Result<(), String> {
    if !path.starts_with('/') || path.contains("..") || path.ends_with('/') {
        return Err(format!("配置文件路径不对：{path}"));
    }
    let shim = crate::site_vps::shim(&ssh.id)?;
    let target = format!("{}@{}", ssh.ssh_user, ssh.ssh_host);
    let ssh_bin = shim.bin_dir().join("ssh");
    let port = ssh.ssh_port.to_string();
    let q = shell_quote(path);
    let read = tokio::process::Command::new(&ssh_bin)
        .args(["-p", &port, &target, &format!("cat {q} 2>/dev/null || true")])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("启动 ssh 失败：{e}"))?;
    if !read.status.success() {
        return Err(format!("读取服务器上的 {path} 失败：{}（先在站点设置里为部署授权）", String::from_utf8_lossy(&read.stderr).trim()));
    }
    let merged = merge_env_file(&String::from_utf8_lossy(&read.stdout), set, remove);
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
    fn ga_variable_is_found_by_name() {
        let v = |k: &str| ConfigVar { key: k.into(), desc: String::new(), secret: false, declared: true, set: false };
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
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }
}
