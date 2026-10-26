//! 站点迁移的命令（规则在 site_migrate / migrate_extract）。
//!
//! 迁移中的站点：`origin = "migrating"`，`migrate_from` 是旧站域名，`domain` 留空——预览部署、上线检查、IndexNow
//! 都不会碰线上域名，切换时才把域名填上。数据都在 `<站点数据>/migrate/`：
//! inventory.json（盘点）、plan.json（方案）、meta.json（各阶段结果）、snapshots/（旧页面快照）、ref/（参考正文）。
//! 仓库里 `.pilot/migrate/`：urls.json（URL 对应表）、assets.json、survey.md（给 AI 的盘点摘要）、screens/（旧站截图）。

use crate::cf::FullRec;
use crate::migrate_extract as mx;
use crate::site_jobs::Job;
use crate::site_migrate::{self as sm, Inventory, Page, Plan};
use crate::sites::Site;
use crate::AppState;
use reqwest::Url;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Manager;

/// 第一步的页数上限：超过就停下并提示。
const MAX_PAGES: usize = 2000;
/// 同时请求几个（对旧站友好一点）。
const PARALLEL: usize = 3;
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36 CligcPilot-Migrate/1.0";
const TRACK_DAYS: u64 = 28;

fn now() -> u64 {
    crate::now_secs()
}

fn mdir(state: &AppState, id: &str) -> PathBuf {
    state.sites.data_dir(id).join("migrate")
}

fn read_json<T: DeserializeOwned>(p: &Path) -> Option<T> {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

fn gz_write(p: &Path, s: &str) -> Result<(), String> {
    let f = std::fs::File::create(p).map_err(|e| e.to_string())?;
    let mut e = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    e.write_all(s.as_bytes()).map_err(|e| e.to_string())?;
    e.finish().map(|_| ()).map_err(|e| e.to_string())
}

fn gz_read(p: &Path) -> Option<String> {
    let f = std::fs::File::open(p).ok()?;
    let mut s = String::new();
    flate2::read::GzDecoder::new(f).read_to_string(&mut s).ok()?;
    Some(s)
}

// ───────────────────────── 状态 ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ExtractStats {
    pub pages: usize,
    pub lists: usize,
    pub assets: usize,
    pub assets_failed: Vec<String>,
    /// 正文可能没抓全的页面
    pub low_coverage: Vec<String>,
    pub collections: BTreeMap<String, usize>,
    /// 超过 25 MB、放到 R2 的文件（站点路径）
    #[serde(default)]
    pub large: Vec<String>,
    /// 抓下来的旧评论条数
    #[serde(default)]
    pub comments: usize,
    /// 要开的动态功能：form / comments / files
    #[serde(default)]
    pub features: Vec<String>,
}

/// 在预览地址（pages.dev）上把每个旧路径请求一遍的结果。
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct LiveCheck {
    pub at: u64,
    pub base: String,
    pub checked: usize,
    pub bad: Vec<String>,
}

/// 把域名接进 Cloudflare 的准备：旧 DNS 记录快照、Zone、名称服务器、缺了哪些记录。
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct DnsPrep {
    pub at: u64,
    pub cf_conn: String,
    pub zone_id: String,
    /// Pilot 帮忙建的 Zone（原来不在 Cloudflare）
    pub created_zone: bool,
    /// 要改成的名称服务器
    pub ns: Vec<String>,
    /// 现在用的名称服务器
    pub current_ns: Vec<String>,
    /// active | pending | …
    pub zone_status: String,
    /// 原来的 DNS 记录（公开查询到的）
    pub snapshot: Vec<FullRec>,
    /// 快照里有、Cloudflare 里还没有的
    pub missing: Vec<FullRec>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Cutover {
    pub at: u64,
    pub host: String,
    pub zone_id: String,
    /// 改 DNS 用的 Cloudflare 连接（自己的服务器：域名在哪个 Cloudflare 账号里）
    #[serde(default)]
    pub cf_conn: String,
    /// 自己的服务器：DNS 是用户自己改的（不在 Cloudflare 里），回退也要用户自己改回去
    #[serde(default)]
    pub manual_dns: bool,
    /// 切换前这个主机名的记录（回退时原样恢复）
    pub before: Vec<FullRec>,
    pub notes: Vec<String>,
    pub rolled_back_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Track {
    pub at: u64,
    pub checked: usize,
    pub bad: Vec<String>,
    /// 切换前 28 天日均点击 / 切换后日均点击（Search Console）
    pub base_clicks: f64,
    pub after_clicks: f64,
    pub after_days: u32,
    pub sitemap_submitted: bool,
    pub indexnow_submitted: bool,
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Meta {
    #[serde(default)]
    pub surveyed_at: u64,
    #[serde(default)]
    pub extracted_at: u64,
    #[serde(default)]
    pub extract: ExtractStats,
    #[serde(default)]
    pub rebuild_at: u64,
    #[serde(default)]
    pub parity_at: u64,
    #[serde(default)]
    pub parity: Vec<mx::ParityCheck>,
    #[serde(default)]
    pub live: LiveCheck,
    #[serde(default)]
    pub dns: DnsPrep,
    #[serde(default)]
    pub cutover: Cutover,
    #[serde(default)]
    pub track: Track,
    /// 正式域名（空＝旧站的域名）。和旧站不一样就是连域名一起搬家，旧域名的 301 要另外做
    #[serde(default)]
    pub formal: String,
    /// 切换前先上线到的测试域名
    #[serde(default)]
    pub staging: Staging,
    /// 换了域名以后，旧域名跳转到新域名
    #[serde(default)]
    pub old_redirect: OldRedirect,
}

/// 旧域名 → 新域名的 301（Cloudflare 重定向规则）。撤销时要原样恢复。
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct OldRedirect {
    pub at: u64,
    /// 跳转的旧主机名（根域名和 www 一起）
    pub from: Vec<String>,
    pub to: String,
    pub zone_id: String,
    pub cf_conn: String,
    /// 为了让规则生效、被 Pilot 打开代理的记录（撤销时关回去）
    pub proxied: Vec<String>,
    /// 设置完实测：旧地址真的 301 到新域名的同一地址
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub note: String,
}

const REDIRECT_TAG: &str = "pilot-migrate";

/// 测试域名：绑在同一个 Pages 项目上，页面不让搜索引擎收录（canonical 仍指向正式域名）。
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Staging {
    pub host: String,
    pub at: u64,
    pub zone_id: String,
    pub cf_conn: String,
    /// Pages 那边的自定义域名状态：pending（验证、签证书中，一般几分钟）/ active
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub live: LiveCheck,
}

/// 旧站实际用的主机名（盘点时跟随跳转后的那个，比如 www.example.com）。
fn origin_host(inv: &Inventory, site: &Site) -> String {
    origin_host_or(inv, &site.migrate_from)
}

fn origin_host_or(inv: &Inventory, fallback: &str) -> String {
    Url::parse(&inv.origin).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or(fallback.to_string())
}

/// 正式域名：改过就用改的，没改就是旧站的主机名。
fn formal_host(inv: &Inventory, site: &Site, m: &Meta) -> String {
    if m.formal.is_empty() { origin_host(inv, site) } else { m.formal.clone() }
}

/// 主机名的基本格式检查（小写、去掉协议和路径）。
fn clean_host(h: &str) -> Result<String, String> {
    let h = h.trim().trim_start_matches("https://").trim_start_matches("http://").split(['/', '?', '#']).next().unwrap_or("").trim_end_matches('.').to_ascii_lowercase();
    let ok = h.contains('.') && h.split('.').all(|l| !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !l.starts_with('-') && !l.ends_with('-'));
    if ok { Ok(h) } else { Err(format!("「{h}」不是有效的域名")) }
}

fn load_meta(dir: &Path) -> Meta {
    read_json(&dir.join("meta.json")).unwrap_or_default()
}

fn save_meta(dir: &Path, f: impl FnOnce(&mut Meta)) -> Result<Meta, String> {
    let mut m = load_meta(dir);
    f(&mut m);
    write_json(&dir.join("meta.json"), &m)?;
    Ok(m)
}

/// 现在走到哪一步：survey 盘点 → plan 确认方案 → extract 抓取 → rebuild 重做 → verify 核对 → cutover 切换 → track 跟踪 → done。
fn stage(inv: Option<&Inventory>, plan: Option<&Plan>, m: &Meta) -> &'static str {
    if m.cutover.at > 0 {
        return if now().saturating_sub(m.cutover.at) > TRACK_DAYS * 86400 { "done" } else { "track" };
    }
    if inv.is_none() || m.surveyed_at == 0 {
        return "survey";
    }
    if plan.is_none_or(|p| p.confirmed_at == 0) {
        return "plan";
    }
    if m.extracted_at == 0 {
        return "extract";
    }
    if m.parity_at == 0 {
        return "rebuild";
    }
    if m.parity.iter().any(|c| c.level == "block") || m.live.at == 0 || !m.live.bad.is_empty() {
        return "verify";
    }
    "cutover"
}

#[derive(Serialize, Clone)]
pub struct TopPage {
    pub path: String,
    pub title: String,
    pub clicks: u64,
    pub impressions: u64,
}

/// 盘点报告（给界面看的摘要）。
#[derive(Serialize, Clone)]
pub struct Summary {
    pub origin: String,
    pub trailing_slash: String,
    pub js_rendered: bool,
    pub truncated: bool,
    pub started_at: u64,
    pub finished_at: u64,
    pub pages: usize,
    pub feeds: usize,
    pub files: usize,
    pub redirects: usize,
    pub broken: Vec<String>,
    pub blocked: usize,
    pub query: usize,
    pub duplicates: usize,
    pub noindex: usize,
    pub assets: usize,
    pub groups: Vec<sm::Group>,
    pub top: Vec<TopPage>,
    pub features: Vec<sm::Feature>,
    pub hosting: sm::Hosting,
    pub analytics: Vec<String>,
    pub generator: String,
    pub gsc_used: bool,
    pub cn_share: Option<f64>,
    pub ownership: sm::Ownership,
    pub notes: Vec<String>,
}

fn summarize(inv: &Inventory) -> Summary {
    let html: Vec<&Page> = inv.html_pages().collect();
    let mut hashes: HashMap<&str, usize> = HashMap::new();
    for p in &html {
        if !p.text_hash.is_empty() && p.words > 50 {
            *hashes.entry(p.text_hash.as_str()).or_default() += 1;
        }
    }
    let mut top: Vec<&Page> = html.iter().copied().filter(|p| p.clicks > 0 || p.impressions > 0).collect();
    top.sort_by(|a, b| b.clicks.cmp(&a.clicks).then(b.impressions.cmp(&a.impressions)));
    Summary {
        origin: inv.origin.clone(),
        trailing_slash: inv.trailing_slash.clone(),
        js_rendered: inv.js_rendered,
        truncated: inv.truncated,
        started_at: inv.started_at,
        finished_at: inv.finished_at,
        pages: html.iter().filter(|p| !p.path.contains('?')).count(),
        feeds: inv.pages.iter().filter(|p| p.kind == "feed").count(),
        files: inv.pages.iter().filter(|p| p.kind == "file").count(),
        redirects: inv.pages.iter().filter(|p| p.kind == "redirect").count(),
        broken: inv.pages.iter().filter(|p| p.kind == "broken").map(|p| format!("{}（{}）", p.path, p.status)).take(100).collect(),
        blocked: inv.pages.iter().filter(|p| p.kind == "blocked").count(),
        query: html.iter().filter(|p| p.path.contains('?')).count(),
        duplicates: hashes.values().filter(|n| **n > 1).map(|n| n - 1).sum(),
        noindex: html.iter().filter(|p| p.noindex).count(),
        assets: inv.assets.len(),
        groups: inv.groups.clone(),
        top: top.into_iter().take(20).map(|p| TopPage { path: p.path.clone(), title: p.title.clone(), clicks: p.clicks, impressions: p.impressions }).collect(),
        features: inv.features.clone(),
        hosting: inv.hosting.clone(),
        analytics: inv.analytics.clone(),
        generator: inv.generator.clone(),
        gsc_used: inv.gsc_used,
        cn_share: inv.cn_share,
        ownership: inv.ownership.clone(),
        notes: inv.notes.clone(),
    }
}

#[derive(Serialize)]
pub struct View {
    pub stage: String,
    pub domain: String,
    pub survey: Option<Summary>,
    pub plan: Option<Plan>,
    pub meta: Meta,
    /// 预览部署好了：pages.dev 地址
    pub preview: String,
    /// 部署方式配好了（Cloudflare 连接 + Pages 项目）
    pub deploy_ready: bool,
    /// 动态功能（表单、评论、大文件）的配置
    pub dynamic: Option<crate::site_dynamic::Dynamic>,
    /// 按确认的方案算出的部署去向（方案里去掉的功能不算）；还没确认方案时是盘点的结论
    pub hosting: Option<sm::Hosting>,
    /// 部署到自己服务器时：服务器的状态（Pilot 托管）
    pub vps: Option<crate::site_vps_host::VpsState>,
    /// 旧站的主机名 / 切换要指过去的正式域名（默认一样）
    pub origin_host: String,
    pub formal: String,
}

#[tauri::command]
pub async fn migrate_state(state: tauri::State<'_, AppState>, id: String) -> Result<View, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Option<Inventory> = read_json(&dir.join("inventory.json"));
    let plan: Option<Plan> = read_json(&dir.join("plan.json"));
    let meta = load_meta(&dir);
    let vps = crate::site_vps_host::load(&state.sites.data_dir(&id)).filter(|_| crate::site_vps_host::managed(&site));
    let deploy_ready = (site.deploy.mode == "direct" && !site.deploy.project.is_empty() && !site.deploy.cf_conn_id.is_empty()) || vps.as_ref().is_some_and(|v| v.setup_at > 0);
    let preview = if !meta.live.base.is_empty() {
        meta.live.base.clone()
    } else if let Some(v) = &vps {
        format!("服务器 {}", v.ip)
    } else if deploy_ready {
        format!("https://{}.pages.dev", site.deploy.project)
    } else {
        String::new()
    };
    let mut meta = meta;
    if meta.cutover.at > 0 && !crate::site_vps_host::managed(&site) {
        if let Some(i) = &inv {
            meta.cutover.notes = direct_notes(&state, &site, i, &meta.cutover.host, &meta);
        }
    }
    let origin = inv.as_ref().map(|i| origin_host(i, &site)).unwrap_or(site.migrate_from.clone());
    let formal = if meta.formal.is_empty() { origin.clone() } else { meta.formal.clone() };
    Ok(View {
        origin_host: origin,
        formal,
        stage: stage(inv.as_ref(), plan.as_ref(), &meta).into(),
        domain: if site.migrate_from.is_empty() { site.domain.clone() } else { site.migrate_from.clone() },
        survey: inv.as_ref().map(summarize),
        meta,
        preview,
        deploy_ready,
        dynamic: crate::site_dynamic::load(&state.sites.data_dir(&id)),
        vps,
        hosting: inv.as_ref().map(|i| sm::hosting_for(i, &plan.as_ref().map(|p| p.features.clone()).unwrap_or_default())),
        plan,
    })
}

/// 确认方案前预览：按选的「保留 / 去掉」重新判断部署去向。
#[tauri::command]
pub async fn migrate_hosting(state: tauri::State<'_, AppState>, id: String, features: BTreeMap<String, String>) -> Result<sm::Hosting, String> {
    let inv: Inventory = read_json(&mdir(&state, &id).join("inventory.json")).ok_or("还没盘点")?;
    Ok(sm::hosting_for(&inv, &features))
}

// ───────────────────────── 抓取工具 ─────────────────────────

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

struct Fetched {
    status: u16,
    location: String,
    ctype: String,
    len: u64,
    body: String,
}

/// 按 Content-Type 或页面里的 meta charset 解码（老的中文站常是 GBK）。
fn decode(bytes: &[u8], ctype: &str) -> String {
    let label = |s: &str| -> Option<String> {
        let l = s.to_ascii_lowercase();
        let i = l.find("charset=")?;
        let rest = &l[i + 8..];
        Some(rest.trim_start_matches(['"', '\'']).split(|c: char| c == '"' || c == '\'' || c == ';' || c == ' ' || c == '>' || c == '/').next().unwrap_or("").to_string())
    };
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let enc = label(ctype).or_else(|| label(&head)).and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes())).unwrap_or(encoding_rs::UTF_8);
    enc.decode(bytes).0.into_owned()
}

async fn fetch(c: &reqwest::Client, url: &str, head: bool) -> Result<Fetched, String> {
    let mut r = if head { c.head(url) } else { c.get(url) }.send().await.map_err(|e| format!("{e}"))?;
    let status = r.status().as_u16();
    let h = r.headers();
    let location = h.get("location").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let ctype = h.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let len = r.content_length().unwrap_or(0);
    let mut buf: Vec<u8> = Vec::new();
    if !head {
        // 网页最多读 8 MB
        while let Some(chunk) = r.chunk().await.map_err(|e| format!("{e}"))? {
            buf.extend_from_slice(&chunk);
            if buf.len() > 8 * 1024 * 1024 {
                break;
            }
        }
    }
    let gz = buf.starts_with(&[0x1f, 0x8b]);
    let body = if gz {
        let mut s = String::new();
        let _ = flate2::read::GzDecoder::new(&buf[..]).read_to_string(&mut s);
        s
    } else {
        decode(&buf, &ctype)
    };
    Ok(Fetched { status, location, ctype, len: if len > 0 { len } else { buf.len() as u64 }, body })
}

/// 跟着跳转走到底（最多 6 次），返回最终网址和响应。
async fn fetch_follow(c: &reqwest::Client, url: &str) -> Result<(Url, Fetched), String> {
    let mut u = Url::parse(url).map_err(|e| e.to_string())?;
    for _ in 0..6 {
        let f = fetch(c, u.as_str(), false).await?;
        if (300..400).contains(&f.status) && !f.location.is_empty() {
            u = u.join(&f.location).map_err(|e| e.to_string())?;
            continue;
        }
        return Ok((u, f));
    }
    Err("跳转次数太多".into())
}

/// 用 DoH 查公开的 DNS 记录。
async fn doh(name: &str, kind: &str) -> Vec<FullRec> {
    let Ok(u) = Url::parse_with_params("https://cloudflare-dns.com/dns-query", &[("name", name), ("type", kind)]) else { return Vec::new() };
    let Ok(r) = reqwest::Client::new()
        .get(u)
        .header("accept", "application/dns-json")
        .timeout(Duration::from_secs(10))
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(v) = r.json::<serde_json::Value>().await else { return Vec::new() };
    let code = match kind {
        "A" => 1,
        "NS" => 2,
        "CNAME" => 5,
        "MX" => 15,
        "TXT" => 16,
        "AAAA" => 28,
        "CAA" => 257,
        _ => 0,
    };
    v["Answer"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["type"].as_u64() == Some(code) && a["name"].as_str().map(|n| n.trim_end_matches('.').eq_ignore_ascii_case(name)).unwrap_or(false))
        .filter_map(|a| {
            let data = a["data"].as_str()?.trim().to_string();
            let (priority, content) = if kind == "MX" {
                let (p, h) = data.split_once(' ')?;
                (p.parse().ok(), h.trim_end_matches('.').to_string())
            } else if kind == "TXT" {
                // DoH 把长 TXT 拆成多个带引号的段
                (None, data.split("\" \"").collect::<Vec<_>>().join("").trim_matches('"').to_string())
            } else {
                (None, data.trim_end_matches('.').to_string())
            };
            Some(FullRec { id: String::new(), kind: kind.into(), name: name.to_string(), content, proxied: false, priority, ttl: a["TTL"].as_u64().unwrap_or(1) as u32 })
        })
        .collect()
}

/// 主机名的根域（example.com）：按最后两段算，常见的二级后缀（com.cn 等）多取一段。
fn apex_of(host: &str) -> String {
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    let n = labels.len();
    if n <= 2 {
        return host.to_string();
    }
    const SLD: &[&str] = &["com", "net", "org", "gov", "edu", "co", "ac"];
    let take = if labels[n - 1].len() == 2 && SLD.contains(&labels[n - 2]) { 3 } else { 2 };
    labels[n.saturating_sub(take)..].join(".")
}

// ───────────────────────── 建迁移站点 ─────────────────────────

#[derive(Deserialize)]
pub struct CreateMigration {
    pub domain: String,
    pub parent_dir: String,
    #[serde(default)]
    pub name: String,
}

/// 首页标题里取站点名：「文章标题 - 站点名」这种取最短的一段，首页常是「站点名 | 口号」。
fn name_from_title(title: &str, domain: &str) -> String {
    let parts: Vec<&str> = title.split(['|', '-', '–', '—', '_', '·']).map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    let pick = parts.first().copied().unwrap_or("");
    let n: String = pick.chars().take(40).collect();
    if n.is_empty() {
        domain.to_string()
    } else {
        n
    }
}

#[tauri::command]
pub async fn create_migration(app: tauri::AppHandle, state: tauri::State<'_, AppState>, req: CreateMigration) -> Result<Site, String> {
    create_one(app, &state, req).await
}

#[derive(Serialize)]
pub struct BatchResult {
    pub domain: String,
    pub site_id: String,
    pub error: String,
}

/// 批量迁移：一次给多个域名，逐个建站点并排队盘点（同时最多 2 个在抓）。某个失败不影响其它的。
#[tauri::command]
pub async fn create_migrations(app: tauri::AppHandle, state: tauri::State<'_, AppState>, domains: Vec<String>, parent_dir: String) -> Result<Vec<BatchResult>, String> {
    let mut seen = std::collections::HashSet::new();
    let list: Vec<String> = domains.iter().map(|d| sm::clean_domain(d)).filter(|d| d.contains('.') && seen.insert(d.trim_start_matches("www.").to_string())).collect();
    if list.is_empty() {
        return Err("没有有效的域名".into());
    }
    if list.len() > 50 {
        return Err("一次最多 50 个".into());
    }
    let mut out = Vec::new();
    for d in list {
        let r = create_one(app.clone(), &state, CreateMigration { domain: d.clone(), parent_dir: parent_dir.clone(), name: String::new() }).await;
        out.push(match r {
            Ok(s) => BatchResult { domain: d, site_id: s.id, error: String::new() },
            Err(e) => BatchResult { domain: d, site_id: String::new(), error: e },
        });
    }
    Ok(out)
}

/// 同时最多几个站在盘点（批量迁移时排队）。
fn survey_slots() -> &'static tokio::sync::Semaphore {
    static S: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    S.get_or_init(|| tokio::sync::Semaphore::new(2))
}

async fn create_one(app: tauri::AppHandle, state: &AppState, req: CreateMigration) -> Result<Site, String> {
    let domain = sm::clean_domain(&req.domain);
    if !domain.contains('.') {
        return Err("请填写要迁移的网站域名，如 example.com".into());
    }
    let apex = domain.trim_start_matches("www.");
    if let Some(s) = state.sites.list().into_iter().find(|s| [&s.domain, &s.migrate_from].iter().any(|d| d.trim_start_matches("www.").eq_ignore_ascii_case(apex))) {
        return Err(format!("「{}」已经是这个域名的站点了", s.name));
    }
    let parent = Path::new(req.parent_dir.trim());
    if !parent.is_absolute() || !parent.is_dir() {
        return Err("存放位置不存在".into());
    }
    // 先确认旧站打得开
    let c = client();
    let (url, home) = match fetch_follow(&c, &format!("https://{domain}/")).await {
        Ok(x) => x,
        Err(_) => fetch_follow(&c, &format!("http://{domain}/")).await.map_err(|e| format!("打不开 {domain}：{e}"))?,
    };
    if home.status >= 400 {
        return Err(format!("{domain} 首页返回 {}，没法迁移", home.status));
    }
    let final_host = url.host_str().unwrap_or(&domain).to_string();
    if !sm::same_site(&final_host, &domain) {
        return Err(format!("{domain} 跳到了别的网站（{final_host}），请填最终的那个域名"));
    }
    let name = if req.name.trim().is_empty() {
        let ex = sm::extract(&home.body, &url, &domain);
        name_from_title(&ex.title, &domain)
    } else {
        req.name.trim().to_string()
    };
    let dir = parent.join(crate::site_make::folder_name(&final_host, &name));
    if dir.exists() && std::fs::read_dir(&dir).map(|mut d| d.next().is_some()).unwrap_or(true) {
        return Err(format!("{} 已经存在且不是空文件夹，换个位置", dir.display()));
    }
    let t = now();
    let mut site = Site {
        id: String::new(),
        name,
        domain: String::new(),
        source: "local".into(),
        gh_conn_id: String::new(),
        repo: String::new(),
        local_dir: dir.to_string_lossy().into_owned(),
        default_branch: "main".into(),
        framework: "astro".into(),
        build: crate::site_make::stack_build("astro").unwrap_or_default(),
        deploy: crate::sites::DeployConfig { mode: "none".into(), ..Default::default() },
        data: crate::sites::DataBinding::default(),
        ops: crate::sites::OpsConfig::default(),
        pinned: false,
        tags: Vec::new(),
        origin: "migrating".into(),
        migrate_from: final_host,
        created_at: t,
        updated_at: t,
    };
    std::fs::create_dir_all(dir.join(".pilot/migrate")).map_err(|e| format!("创建文件夹失败：{e}"))?;
    let cleanup = |e: String| {
        let _ = std::fs::remove_dir_all(&dir);
        e
    };
    crate::launch_cmds::init_repo(&dir).await.map_err(cleanup)?;
    crate::site_cmds::validate(&mut site, state).map_err(cleanup)?;
    let site = state.sites.insert(site).map_err(cleanup)?;
    start_survey(app, state, &site.id)?;
    Ok(site)
}

// ───────────────────────── ① 盘点 ─────────────────────────

const SURVEY_STEPS: &[(&str, &str)] = &[
    ("origin", "连上旧站"),
    ("sitemap", "读 robots.txt 和 sitemap"),
    ("gsc", "用 Search Console 补漏"),
    ("crawl", "抓取页面"),
    ("render", "浏览器渲染（JS 渲染的站）"),
    ("analyze", "分析页面和部署方式"),
    ("screens", "截取旧站页面样式"),
];

#[tauri::command]
pub async fn migrate_survey(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    start_survey(app, &state, &id)
}

fn start_survey(app: tauri::AppHandle, state: &AppState, id: &str) -> Result<String, String> {
    let site = state.sites.get(id)?;
    if site.migrate_from.is_empty() {
        return Err("这个站点不在迁移中".into());
    }
    let job = state.site_jobs.start(Some(app), id, "migrate:survey", "盘点旧站", SURVEY_STEPS)?;
    let job_id = job.id.clone();
    tauri::async_runtime::spawn(async move {
        // 批量迁移时排队：同时最多 2 个站在抓
        job.step("origin", "running", "排队中…");
        let _slot = survey_slots().acquire().await;
        let r = run_survey(site, &job).await;
        job.finish(&r);
    });
    Ok(job_id)
}

fn app_state() -> Result<tauri::State<'static, AppState>, String> {
    Ok(crate::APP.get().ok_or("应用还没准备好")?.state::<AppState>())
}

struct Crawl {
    c: reqwest::Client,
    origin: Url,
    domain: String,
    robots: sm::Robots,
    snap_dir: PathBuf,
    pages: BTreeMap<String, Page>,
    found: BTreeMap<String, String>,
    queue: VecDeque<String>,
    links_in: HashMap<String, u32>,
    assets: BTreeMap<String, u32>,
    analytics: Vec<String>,
    generator: String,
    truncated: bool,
}

impl Crawl {
    fn enqueue(&mut self, path: String, how: &str) {
        if sm::is_junk(&path) || sm::is_asset(&path) {
            return;
        }
        let p = path.split('?').next().unwrap_or("");
        if p.starts_with("/sitemap") && p.ends_with(".xml") || p == "/robots.txt" {
            return;
        }
        let e = self.found.entry(path.clone()).or_default();
        if !e.split(',').any(|x| x == how) {
            if !e.is_empty() {
                e.push(',');
            }
            e.push_str(how);
        }
        if !self.pages.contains_key(&path) && !self.queue.contains(&path) {
            if self.pages.len() + self.queue.len() >= MAX_PAGES * 2 {
                self.truncated = true;
                return;
            }
            self.queue.push_back(path);
        }
    }

    fn url_of(&self, path: &str) -> String {
        format!("{}{}", self.origin.as_str().trim_end_matches('/'), path)
    }

    /// 处理抓到的一页网页（原始的或浏览器渲染过的）。
    fn take_html(&mut self, page: &mut Page, html: &str, rendered: bool) {
        let url = Url::parse(&self.url_of(&page.path)).unwrap_or_else(|_| self.origin.clone());
        let ex = sm::extract(html, &url, &self.domain);
        let words = sm::word_count(&ex.text);
        page.kind = "page".into();
        page.title = ex.title.clone();
        page.description = ex.description.clone();
        page.canonical = ex.canonical.clone();
        page.h1 = ex.h1.clone();
        page.lang = ex.lang.clone();
        page.hreflang = ex.hreflang.clone();
        page.og_type = ex.og_type.clone();
        page.og_image = ex.og_image.clone();
        page.schema = ex.schema.clone();
        page.published = ex.published.clone();
        page.modified = ex.modified.clone();
        page.author = ex.author.clone();
        page.tags = ex.tags.clone();
        page.noindex = ex.noindex;
        page.words = words;
        page.links_out = ex.links.len() as u32;
        page.text_hash = sm::short_hash(&ex.text);
        page.group = sm::group_of(&page.path);
        page.ptype = sm::page_type(&page.path, &ex, words).into();
        page.features = ex.features.clone();
        page.rendered = rendered;
        let snap = format!("{}.html.gz", sm::short_hash(&page.path));
        if gz_write(&self.snap_dir.join(&snap), html).is_ok() {
            page.snapshot = snap;
        }
        for a in &ex.assets {
            *self.assets.entry(a.clone()).or_default() += 1;
        }
        for id in ex.analytics {
            if !self.analytics.contains(&id) {
                self.analytics.push(id);
            }
        }
        if self.generator.is_empty() && !ex.generator.is_empty() {
            self.generator = ex.generator;
        }
        for l in ex.links {
            *self.links_in.entry(l.clone()).or_default() += 1;
            self.enqueue(l, "crawl");
        }
    }
}

/// 抓一个路径（不跟跳转），返回 (Page, 网页 HTML)。
async fn crawl_one(c: reqwest::Client, base: String, path: String, domain: String) -> (Page, Option<String>) {
    let mut p = Page { path: path.clone(), ..Default::default() };
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let head = sm::is_file(&path);
    let f = match fetch(&c, &url, head).await {
        Ok(f) if head && (f.status == 405 || f.status == 403) => fetch(&c, &url, false).await,
        x => x,
    };
    let f = match f {
        Ok(f) => f,
        Err(e) => {
            p.kind = "broken".into();
            p.title = e;
            return (p, None);
        }
    };
    p.status = f.status;
    p.content_type = f.ctype.clone();
    p.bytes = f.len;
    if (300..400).contains(&f.status) {
        p.kind = "redirect".into();
        if let Ok(u) = Url::parse(&url).and_then(|b| b.join(&f.location)) {
            p.redirect_to = if sm::same_site(u.host_str().unwrap_or(""), &domain) { sm::path_of(&u) } else { u.to_string() };
        }
        return (p, None);
    }
    if f.status >= 400 {
        p.kind = "broken".into();
        return (p, None);
    }
    let xml = f.ctype.contains("xml") || f.body.trim_start().starts_with("<?xml");
    if xml && (f.body.contains("<rss") || f.body.contains("<feed")) || f.ctype.contains("rss") || f.ctype.contains("atom") {
        p.kind = "feed".into();
        return (p, None);
    }
    if head || !(f.ctype.contains("html") || (f.ctype.is_empty() && f.body.trim_start().starts_with('<'))) {
        p.kind = "file".into();
        return (p, None);
    }
    (p, Some(f.body))
}

/// 用 render.cjs 渲染一批路径，返回 路径 → 渲染后的 HTML。
async fn render_batch(origin: &Url, paths: &[String], job: &Job) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(state) = app_state() else { return out };
    let Ok(script) = crate::tools::ensure_render(&state.data_dir) else { return out };
    let Ok(tmp) = tempfile::tempdir() else { return out };
    let urls: Vec<String> = paths.iter().map(|p| format!("{}{}", origin.as_str().trim_end_matches('/'), p)).collect();
    let list = tmp.path().join("urls.json");
    if std::fs::write(&list, serde_json::to_vec(&urls).unwrap_or_default()).is_err() {
        return out;
    }
    let node = crate::brains::resolve_bin("node");
    let args = vec![script.to_string_lossy().into_owned(), "--list".into(), list.to_string_lossy().into_owned(), "--out".into(), tmp.path().join("out").to_string_lossy().into_owned()];
    let r = tokio::time::timeout(
        Duration::from_secs(60 + paths.len() as u64 * 40),
        tokio::process::Command::new(&node).args(&args).kill_on_drop(true).output(),
    )
    .await;
    match r {
        Ok(Ok(o)) => {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
                if v["ok"].as_bool() == Some(true) {
                    if let Some(i) = v["i"].as_u64() {
                        if let Ok(html) = std::fs::read_to_string(tmp.path().join("out").join(format!("{i}.html"))) {
                            out.insert(paths[i as usize].clone(), html);
                        }
                    }
                } else if v.get("i").is_none() {
                    job.line(&format!("浏览器渲染失败：{}", v["error"].as_str().unwrap_or("")));
                }
            }
        }
        Ok(Err(e)) => job.line(&format!("启动浏览器渲染失败（需要 Node 和 Chrome）：{e}")),
        Err(_) => job.line("浏览器渲染超时"),
    }
    out
}

async fn run_survey(site: Site, job: &Job) -> Result<(), String> {
    let state = app_state()?;
    let domain = site.migrate_from.clone();
    let dir = mdir(&state, &site.id);
    let snap_dir = dir.join("snapshots");
    let _ = std::fs::remove_dir_all(&snap_dir);
    std::fs::create_dir_all(&snap_dir).map_err(|e| e.to_string())?;
    let started = now();
    let c = client();

    // 连上旧站：跟着跳转找到实际用的协议和主机（http → https、apex ↔ www）
    job.step("origin", "running", &domain);
    let (home_url, _) = match fetch_follow(&c, &format!("https://{domain}/")).await {
        Ok(x) => x,
        Err(_) => fetch_follow(&c, &format!("http://{domain}/")).await.map_err(|e| format!("打不开 {domain}：{e}"))?,
    };
    let origin = Url::parse(&format!("{}://{}/", home_url.scheme(), home_url.host_str().unwrap_or(&domain))).map_err(|e| e.to_string())?;
    job.step("origin", "ok", origin.as_str());

    // 顺手按旧站域名接上 Search Console / GA4（同一个账号里找到就绑）
    let _ = crate::launch_cmds::probe_bindings(&state, &site, true).await;
    let site = state.sites.get(&site.id).unwrap_or(site);

    let mut cr = Crawl {
        c: c.clone(),
        origin: origin.clone(),
        domain: domain.clone(),
        robots: sm::Robots::default(),
        snap_dir: snap_dir.clone(),
        pages: BTreeMap::new(),
        found: BTreeMap::new(),
        queue: VecDeque::new(),
        links_in: HashMap::new(),
        assets: BTreeMap::new(),
        analytics: Vec::new(),
        generator: String::new(),
        truncated: false,
    };
    let mut notes: Vec<String> = Vec::new();

    // robots.txt + sitemap
    job.step("sitemap", "running", "");
    if let Ok(f) = fetch(&c, &cr.url_of("/robots.txt"), false).await {
        if f.status == 200 && !f.ctype.contains("html") {
            cr.robots = sm::parse_robots(&f.body);
        }
    }
    // 首页最先抓
    cr.enqueue("/".into(), "crawl");
    let mut maps: Vec<String> = cr.robots.sitemaps.clone();
    for p in ["/sitemap.xml", "/sitemap_index.xml", "/sitemap-index.xml"] {
        let u = cr.url_of(p);
        if !maps.contains(&u) {
            maps.push(u);
        }
    }
    let (mut seen_maps, mut from_map) = (HashSet::new(), 0usize);
    while let Some(m) = maps.pop() {
        if !seen_maps.insert(m.clone()) || seen_maps.len() > 60 || job.canceled() {
            continue;
        }
        let Ok((_, f)) = fetch_follow(&c, &m).await else { continue };
        if f.status != 200 {
            continue;
        }
        let (urls, subs) = sm::parse_sitemap(&f.body);
        // 图片 / 视频 sitemap 列的是附件页（WordPress 的 attachment），不是要迁的页面
        maps.extend(subs.into_iter().filter(|s| !sm::is_media_sitemap(s)));
        if sm::is_media_sitemap(&m) {
            continue;
        }
        for u in urls {
            if let Ok(u) = Url::parse(&u) {
                if sm::same_site(u.host_str().unwrap_or(""), &domain) {
                    cr.enqueue(sm::path_of(&u), "sitemap");
                    from_map += 1;
                }
            }
        }
    }
    job.step("sitemap", "ok", &format!("sitemap 里 {from_map} 个地址"));

    // Search Console：近 16 个月有展示的页面
    let mut traffic: HashMap<String, (u64, u64)> = HashMap::new();
    let mut gsc_used = false;
    let mut cn_share = None;
    if site.data.gsc_site.is_empty() {
        job.step("gsc", "skipped", "没有接 Search Console（接上能把有排名的页面一个不漏地找出来）");
    } else {
        job.step("gsc", "running", &site.data.gsc_site);
        let end = chrono::Local::now().date_naive() - chrono::Duration::days(2);
        let start = end - chrono::Duration::days(480);
        match crate::site_data::google_token(&state.conns, site.data.gsc_conn(), &crate::google::read_scopes()).await {
            Ok(token) => match crate::google::gsc_query(&token, &site.data.gsc_site, &start.to_string(), &end.to_string(), &["page"], 25000).await {
                Ok(rows) => {
                    gsc_used = true;
                    let mut n = 0;
                    for r in rows {
                        let Some(u) = r.keys.first().and_then(|k| Url::parse(k).ok()) else { continue };
                        if !sm::same_site(u.host_str().unwrap_or(""), &domain) {
                            continue;
                        }
                        let p = sm::path_of(&u);
                        let e = traffic.entry(p.clone()).or_default();
                        e.0 += r.clicks as u64;
                        e.1 += r.impressions as u64;
                        cr.enqueue(p, "gsc");
                        n += 1;
                    }
                    job.step("gsc", "ok", &format!("{n} 个有展示的页面"));
                }
                Err(e) => job.step("gsc", "failed", &e),
            },
            Err(e) => job.step("gsc", "failed", &e),
        }
        // 访客地区（GA4）
        if !site.data.ga4_property.is_empty() {
            if let Ok(token) = crate::site_data::google_token(&state.conns, &site.data.google_conn_id, &crate::google::read_scopes()).await {
                let end = chrono::Local::now().date_naive() - chrono::Duration::days(1);
                let start = end - chrono::Duration::days(27);
                if let Ok(rows) = crate::google::ga4_report(&token, &site.data.ga4_property, &start.to_string(), &end.to_string(), &["countryId"], &["totalUsers"], 100, None).await {
                    let total: f64 = rows.iter().map(|r| r.1.first().copied().unwrap_or(0.0)).sum();
                    let cn: f64 = rows.iter().filter(|r| r.0.first().map(|c| c == "CN").unwrap_or(false)).map(|r| r.1.first().copied().unwrap_or(0.0)).sum();
                    if total > 0.0 {
                        cn_share = Some(cn / total);
                    }
                }
            }
        }
    }

    // 抓页面
    job.step("crawl", "running", "");
    let delay = Duration::from_millis(cr.robots.crawl_delay.map(|d| (d.min(5.0) * 1000.0) as u64).unwrap_or(250));
    let mut js_rendered = false;
    let mut probe_html: Vec<String> = Vec::new();
    let mut to_render: Vec<String> = Vec::new();
    let mut raw_html: HashMap<String, String> = HashMap::new();
    let mut rendered_n = 0usize;
    let mut probed = 0usize;
    while !cr.queue.is_empty() {
        if job.canceled() {
            return Err("已取消".into());
        }
        let mut set = tokio::task::JoinSet::new();
        while set.len() < PARALLEL {
            let Some(path) = cr.queue.pop_front() else { break };
            if cr.pages.len() + set.len() >= MAX_PAGES {
                cr.truncated = true;
                cr.queue.clear();
                break;
            }
            if !cr.robots.allowed(&path) {
                cr.pages.insert(path.clone(), Page { kind: "blocked".into(), group: sm::group_of(&path), ..Page { path, ..Default::default() } });
                continue;
            }
            set.spawn(crawl_one(cr.c.clone(), origin.to_string(), path, domain.clone()));
        }
        while let Some(r) = set.join_next().await {
            let Ok((mut page, html)) = r else { continue };
            if let Some(html) = html {
                // 前几页判断是不是 JS 渲染的站
                if !js_rendered && probed < 5 {
                    probed += 1;
                    let doc = scraper::Html::parse_document(&html);
                    let words = doc.select(&scraper::Selector::parse("body").unwrap()).next().map(|b| sm::word_count(&sm::visible_text(b))).unwrap_or(0);
                    if sm::looks_client_rendered(&html, words) {
                        probe_html.push(page.path.clone());
                        if probe_html.len() >= 2 {
                            js_rendered = true;
                            job.line("页面内容是浏览器端渲染的，改用浏览器抓取");
                        }
                    }
                }
                if js_rendered {
                    raw_html.insert(page.path.clone(), html);
                    to_render.push(page.path.clone());
                    cr.pages.insert(page.path.clone(), page);
                    continue;
                }
                cr.take_html(&mut page, &html, false);
            } else if page.kind == "redirect" && !page.redirect_to.is_empty() && page.redirect_to.starts_with('/') {
                let t = page.redirect_to.clone();
                cr.enqueue(t, "crawl");
            }
            page.group = if page.group.is_empty() { sm::group_of(&page.path) } else { page.group };
            cr.pages.insert(page.path.clone(), page);
        }
        // JS 渲染站：攒一批交给浏览器（前面已经抓过的几页也补上）
        if js_rendered && (to_render.len() >= 10 || cr.queue.is_empty()) && !to_render.is_empty() {
            for p in probe_html.drain(..) {
                if !to_render.contains(&p) {
                    to_render.push(p);
                }
            }
            job.step("render", "running", &format!("已渲染 {rendered_n} 页"));
            let batch: Vec<String> = std::mem::take(&mut to_render);
            let got = render_batch(&origin, &batch, job).await;
            for p in batch {
                let Some(mut page) = cr.pages.remove(&p) else { continue };
                match got.get(&p).or(raw_html.get(&p)) {
                    Some(h) => {
                        let h = h.clone();
                        cr.take_html(&mut page, &h, got.contains_key(&p));
                        rendered_n += 1;
                    }
                    // 渲染失败：前面按原始 HTML 处理过的保留原样，从没处理过的才算打不开
                    None if page.kind.is_empty() => page.kind = "broken".into(),
                    None => {}
                }
                raw_html.remove(&p);
                cr.pages.insert(p, page);
            }
        }
        job.step("crawl", "running", &format!("已抓 {} 个，待抓 {}", cr.pages.len(), cr.queue.len()));
        tokio::time::sleep(delay).await;
    }
    let html_n = cr.pages.values().filter(|p| p.kind == "page").count();
    job.step("crawl", "ok", &format!("{} 个地址，其中网页 {html_n} 个", cr.pages.len()));
    if js_rendered {
        job.step("render", "ok", &format!("浏览器渲染了 {rendered_n} 页"));
    } else {
        job.step("render", "skipped", "页面是服务端生成的，不需要浏览器渲染");
    }

    // 分析
    job.step("analyze", "running", "");
    let mut pages: Vec<Page> = std::mem::take(&mut cr.pages).into_values().collect();
    for p in pages.iter_mut() {
        p.found = cr.found.get(&p.path).cloned().unwrap_or_default();
        p.links_in = cr.links_in.get(&p.path).copied().unwrap_or(0);
        if let Some((c, i)) = traffic.get(&p.path) {
            p.clicks = *c;
            p.impressions = *i;
        }
    }
    let trailing = sm::slash_style(pages.iter().filter(|p| p.kind == "page").map(|p| p.path.as_str())).to_string();
    let title_suffix = sm::tidy_titles(&mut pages);
    let groups = sm::summarize_groups(&pages);
    let features = sm::summarize_features(&pages);
    let largest = pages.iter().filter(|p| p.kind == "file").map(|p| p.bytes).max().unwrap_or(0);
    let hosting = sm::decide_hosting(&sm::HostingInput { pages: html_n, assets: cr.assets.len(), largest_file: largest, features: &features, cn_share });
    if cr.truncated {
        notes.push(format!("页面超过 {MAX_PAGES} 个，只盘点了前 {MAX_PAGES} 个。这一版先支持 {MAX_PAGES} 页以内的站"));
    }
    let blocked = pages.iter().filter(|p| p.kind == "blocked").count();
    if blocked > 0 {
        notes.push(format!("{blocked} 个地址被 robots.txt 禁止抓取，没有盘点"));
    }
    let query = pages.iter().filter(|p| p.kind == "page" && p.path.contains('?')).count();
    if query > 0 {
        notes.push(format!("{query} 个带查询参数的网页（如 ?page=2）：静态站按路径出页面，这类地址不迁"));
    }
    if trailing == "mixed" {
        notes.push("旧站的地址有的以 / 结尾、有的不以 / 结尾：新站会逐个照原样生成".into());
    }
    if !gsc_used {
        notes.push("没有用 Search Console 补漏：没被链接到、也不在 sitemap 里的页面可能漏掉。接上 Search Console 后重新盘点更稳".into());
    }
    // 所有权线索
    let mut own = sm::Ownership { txt_token: format!("pilot-verify={}", &uuid::Uuid::new_v4().simple().to_string()[..20]), ..Default::default() };
    if let Some(prev) = read_json::<Inventory>(&dir.join("inventory.json")) {
        own.txt_token = prev.ownership.txt_token;
    }
    if !site.data.gsc_site.is_empty() {
        own.gsc_site = site.data.gsc_site.clone();
    }
    for cf in state.conns.list().into_iter().filter(|c| c.kind == "cloudflare") {
        let Ok(token) = crate::keychain::get_key(&cf.id) else { continue };
        if let Ok(Some(z)) = crate::cf::zone_for(&token, &domain).await {
            own.cf_conn = cf.id.clone();
            own.cf_zone = z.id.clone();
            if let Ok((status, _)) = crate::cf::zone_info(&token, &z.id).await {
                if status == "active" {
                    own.verified = true;
                    own.by = "cloudflare".into();
                }
            }
            break;
        }
    }
    if !own.verified && !own.gsc_site.is_empty() {
        own.verified = true;
        own.by = "gsc".into();
    }
    // 只保留页面引用的内容图片和附件（页面上的主题样式、脚本不算）
    let assets: BTreeMap<String, u32> = cr.assets.into_iter().filter(|(u, _)| Url::parse(u).map(|u| !sm::ext_of(u.path()).is_empty()).unwrap_or(false)).collect();
    let inv = Inventory {
        domain: domain.clone(),
        origin: origin.as_str().trim_end_matches('/').to_string(),
        trailing_slash: trailing,
        js_rendered,
        started_at: started,
        finished_at: now(),
        truncated: cr.truncated,
        groups,
        features,
        hosting,
        analytics: cr.analytics,
        generator: cr.generator,
        assets,
        gsc_used,
        cn_share,
        ownership: own,
        notes,
        title_suffix,
        pages,
    };
    write_json(&dir.join("inventory.json"), &inv)?;
    // 盘点重做了：之前的方案和后面各步作废
    let _ = std::fs::remove_file(dir.join("plan.json"));
    save_meta(&dir, |m| {
        *m = Meta { surveyed_at: now(), ..Default::default() };
    })?;
    job.step("analyze", "ok", &inv.hosting.label);

    // 截几张旧站的样子（首页 + 页数最多的几组各一页），重做时当视觉参考
    job.step("screens", "running", "");
    let code = PathBuf::from(&site.local_dir);
    let shots_dir = code.join(".pilot/migrate/screens");
    let _ = std::fs::create_dir_all(&shots_dir);
    let mut targets: Vec<(String, String)> = vec![("home".into(), "/".into())];
    for g in inv.groups.iter().filter(|g| g.key != "/").take(4) {
        if let Some(s) = g.samples.first() {
            targets.push((sm::short_hash(&g.key)[..8].to_string(), s.clone()));
        }
    }
    let mut shot_n = 0;
    if let Ok(shot) = crate::tools::ensure_shot(&state.data_dir) {
        let node = crate::brains::resolve_bin("node");
        for (name, path) in &targets {
            if job.canceled() {
                break;
            }
            let out = shots_dir.join(format!("{name}.png"));
            let url = format!("{}{}", inv.origin, path);
            let r = tokio::time::timeout(
                Duration::from_secs(60),
                tokio::process::Command::new(&node).arg(&shot).args(["--url", &url, "--out"]).arg(&out).args(["--width", "1280", "--height", "900"]).kill_on_drop(true).output(),
            )
            .await;
            if matches!(r, Ok(Ok(ref o)) if o.status.success()) {
                shot_n += 1;
            }
        }
    }
    if shot_n > 0 {
        job.step("screens", "ok", &format!("{shot_n} 张"));
    } else {
        job.step("screens", "skipped", "没截到（需要本机有 Chrome）");
    }
    Ok(())
}

// ───────────────────────── ② 确认方案 ─────────────────────────

#[tauri::command]
pub async fn migrate_save_plan(state: tauri::State<'_, AppState>, id: String, plan: Plan) -> Result<Plan, String> {
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    let features: BTreeMap<String, String> = plan.features.into_iter().filter(|(k, v)| inv.features.iter().any(|f| &f.key == k) && matches!(v.as_str(), "keep" | "drop")).collect();
    let hosting = sm::hosting_for(&inv, &features);
    // 登录 / 购物车这类保留下来的话只能放服务器；其它的按用户选（大陆访客多时建议服务器）
    let deploy = if hosting.target == "vps" || plan.deploy == "vps" { "vps" } else { "direct" };
    if deploy == "vps" && features.iter().any(|(k, v)| matches!(k.as_str(), "login" | "cart") && v != "drop") {
        return Err("登录、购物车这类功能 Pilot 还不能在服务器上重做：在方案里选「去掉」，或者迁移后另外接入".into());
    }
    let plan = Plan {
        features,
        stack: "astro".into(),
        deploy: deploy.into(),
        visual: if plan.visual == "fresh" { "fresh".into() } else { "keep_brand".into() },
        style: plan.style.trim().chars().take(500).collect(),
        drop_groups: plan.drop_groups,
        drop_paths: plan.drop_paths,
        confirmed_at: now(),
    };
    write_json(&dir.join("plan.json"), &plan)?;
    // 方案改了，抓取要重来
    save_meta(&dir, |m| {
        m.extracted_at = 0;
        m.parity_at = 0;
        m.parity.clear();
        m.live = LiveCheck::default();
    })?;
    Ok(plan)
}

// ───────────────────────── ③ 抓取 ─────────────────────────

const EXTRACT_STEPS: &[(&str, &str)] = &[("content", "提取正文"), ("assets", "下载图片和附件"), ("files", "写入项目和 URL 对应表"), ("commit", "提交")];

#[tauri::command]
pub async fn migrate_extract(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    let plan: Plan = read_json(&dir.join("plan.json")).filter(|p: &Plan| p.confirmed_at > 0).ok_or("先确认迁移方案")?;
    seed_ga(&state, &id, &inv)?;
    let job = state.site_jobs.start(Some(app), &id, "migrate:extract", "抓取旧站内容", EXTRACT_STEPS)?;
    let job_id = job.id.clone();
    tauri::async_runtime::spawn(async move {
        let r = run_extract(site, inv, plan, dir, &job).await;
        job.finish(&r);
    });
    Ok(job_id)
}

/// 旧站的 GA4 衡量 ID 直接存成配置变量 `PUBLIC_GA_ID`：新站沿用同一个统计，不用 AI 写死、也不用再问我。
/// 已经有值（我自己填过）就不动。
fn seed_ga(state: &AppState, id: &str, inv: &Inventory) -> Result<(), String> {
    let Some(ga) = inv.analytics.iter().find(|a| a.starts_with("G-")) else { return Ok(()) };
    if crate::sites::load_env(id).iter().any(|(k, _)| k == "PUBLIC_GA_ID") {
        return Ok(());
    }
    let keys = crate::sites::save_env(id, &[("PUBLIC_GA_ID".into(), ga.clone())], &[])?;
    state.sites.update(id, now(), |s| {
        s.build.env_keys = keys;
        Ok(())
    })?;
    Ok(())
}

async fn run_extract(site: Site, inv: Inventory, plan: Plan, dir: PathBuf, job: &Job) -> Result<(), String> {
    let code = PathBuf::from(&site.local_dir);
    let snap = dir.join("snapshots");
    let refs = dir.join("ref");
    let _ = std::fs::remove_dir_all(&refs);
    std::fs::create_dir_all(&refs).map_err(|e| e.to_string())?;
    let content_root = code.join("src/content");

    // 模板块：用全部网页算
    job.step("content", "running", "识别模板（导航、页脚、侧栏）");
    let html_pages: Vec<&Page> = inv.html_pages().filter(|p| !p.snapshot.is_empty()).collect();
    let mut sigs = Vec::new();
    for p in &html_pages {
        if let Some(h) = gz_read(&snap.join(&p.snapshot)) {
            sigs.push((p.group.clone(), mx::block_sigs(&h)));
        }
    }
    let boiler = mx::boilerplate(&sigs);
    drop(sigs);

    let mut stats = ExtractStats::default();
    let mut files: HashMap<String, String> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    let mut assets: BTreeMap<String, String> = BTreeMap::new();
    let total = html_pages.len();
    for (i, p) in html_pages.iter().enumerate() {
        if job.canceled() {
            return Err("已取消".into());
        }
        if !plan.keeps(p) || p.path.contains('?') {
            continue;
        }
        if i % 20 == 0 {
            job.step("content", "running", &format!("{i} / {total}"));
        }
        let Some(html) = gz_read(&snap.join(&p.snapshot)) else { continue };
        let url = Url::parse(&format!("{}{}", inv.origin, p.path)).map_err(|e| e.to_string())?;
        // og:image 也下载下来
        let og = Url::parse(&p.og_image).ok().or_else(|| url.join(&p.og_image).ok()).filter(|_| !p.og_image.is_empty());
        let og_local = og.map(|u| {
            let l = mx::local_asset(&u, &inv.domain);
            assets.insert(u.to_string(), l.clone());
            l
        });
        if matches!(p.ptype.as_str(), "home" | "list") {
            stats.lists += 1;
            continue;
        }
        let c = mx::extract_content(&html, &url, &inv.domain, &boiler);
        if c.markdown.trim().is_empty() {
            stats.low_coverage.push(format!("{}（没取到正文）", p.path));
            continue;
        }
        if c.coverage < 0.6 {
            stats.low_coverage.push(format!("{}（约 {:.0}%）", p.path, c.coverage * 100.0));
        }
        let (col, rel) = mx::content_path(&p.path);
        let mut file = format!("{col}/{rel}");
        let mut n = 2;
        while !used.insert(file.to_ascii_lowercase()) {
            file = format!("{col}/{rel}-{n}");
            n += 1;
        }
        let abs = content_root.join(format!("{file}.md"));
        if let Some(d) = abs.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&abs, format!("{}{}", mx::frontmatter(p, og_local.as_deref().unwrap_or(""), &inv.title_suffix), c.markdown)).map_err(|e| e.to_string())?;
        std::fs::write(refs.join(mx::ref_name(&p.path)), &c.text).map_err(|e| e.to_string())?;
        files.insert(p.path.clone(), format!("src/content/{file}.md"));
        *stats.collections.entry(col).or_default() += 1;
        stats.pages += 1;
        assets.extend(c.assets);
    }
    job.step("content", "ok", &format!("{} 篇内容，{} 个列表 / 首页", stats.pages, stats.lists));

    // 下载图片和附件（本站的附件 URL 也要原样保留）+ favicon
    job.step("assets", "running", "");
    for p in inv.pages.iter().filter(|p| p.kind == "file" && plan.keeps(p)) {
        if let Ok(u) = Url::parse(&format!("{}{}", inv.origin, p.path)) {
            assets.insert(u.to_string(), p.path.clone());
        }
    }
    for f in ["/favicon.ico", "/apple-touch-icon.png"] {
        if let Ok(u) = Url::parse(&format!("{}{}", inv.origin, f)) {
            assets.entry(u.to_string()).or_insert_with(|| f.to_string());
        }
    }
    let public = code.join("public");
    let c = client();
    let list: Vec<(String, String)> = assets.clone().into_iter().collect();
    let mut done = 0usize;
    for chunk in list.chunks(4) {
        if job.canceled() {
            return Err("已取消".into());
        }
        let mut set = tokio::task::JoinSet::new();
        for (url, local) in chunk {
            let (c, url, local, public, large) = (c.clone(), url.clone(), local.clone(), public.clone(), dir.join("large"));
            set.spawn(async move {
                let dest = public.join(mx::pct_decode(local.trim_start_matches('/')));
                if dest.is_file() {
                    return (local, Ok::<bool, String>(false));
                }
                if large.join(mx::pct_decode(local.trim_start_matches('/'))).is_file() {
                    return (local, Ok(true));
                }
                let r = async {
                    let (_, f) = fetch_follow_bytes(&c, &url).await?;
                    // 超过 25 MB 的 Pages 放不下：先存在站点数据里，配置动态功能时传到 R2
                    let big = f.len() as u64 > sm::CF_MAX_FILE;
                    let dest = if big { large.join(mx::pct_decode(local.trim_start_matches('/'))) } else { dest };
                    if let Some(d) = dest.parent() {
                        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&dest, &f).map_err(|e| e.to_string())?;
                    Ok(big)
                }
                .await;
                (local, r)
            });
        }
        while let Some(r) = set.join_next().await {
            let Ok((local, r)) = r else { continue };
            match r {
                Ok(big) => {
                    stats.assets += 1;
                    if big {
                        stats.large.push(local);
                    }
                }
                Err(e) => {
                    if !local.starts_with("/favicon") && !local.starts_with("/apple-touch") {
                        stats.assets_failed.push(format!("{local}（{e}）"));
                    }
                }
            }
        }
        done += chunk.len();
        job.step("assets", "running", &format!("{done} / {}", list.len()));
    }
    job.step("assets", "ok", &format!("{} 个，失败 {} 个", stats.assets, stats.assets_failed.len()));

    // 动态功能：按方案保留表单 / 评论，有大文件就用 R2。旧评论抓下来静态保留，Functions 代码写进项目
    job.step("files", "running", "");
    let has_feature = |k: &str| inv.features.iter().any(|f| f.key == k) && plan.keeps_feature(k);
    if has_feature("form") {
        stats.features.push("form".into());
    }
    if has_feature("comments") {
        stats.features.push("comments".into());
        let mut all: BTreeMap<String, Vec<mx::OldComment>> = BTreeMap::new();
        for p in inv.html_pages().filter(|p| plan.keeps(p) && !p.snapshot.is_empty() && p.features.iter().any(|f| f == "comments")) {
            if let Some(h) = gz_read(&snap.join(&p.snapshot)) {
                let cs = mx::extract_comments(&h);
                if !cs.is_empty() {
                    stats.comments += cs.len();
                    all.insert(p.path.clone(), cs);
                }
            }
        }
        let f = code.join("src/data/comments.json");
        if all.is_empty() {
            let _ = std::fs::remove_file(&f);
        } else {
            write_json(&f, &all)?;
        }
    }
    if !stats.large.is_empty() {
        stats.features.push("files".into());
    }
    let site_dir = dir.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.clone());
    if !stats.features.is_empty() {
        let mut d = crate::site_dynamic::load(&site_dir).unwrap_or_default();
        d.features = stats.features.clone();
        d.large = stats.large.clone();
        crate::site_dynamic::write_code(&site.app_dir(&code), &d)?;
        crate::site_dynamic::save(&site_dir, &d)?;
    } else if let Some(mut d) = crate::site_dynamic::load(&site_dir) {
        // 方案改成都不要了：去掉代码
        d.features.clear();
        d.large.clear();
        crate::site_dynamic::write_code(&site.app_dir(&code), &d)?;
        crate::site_dynamic::save(&site_dir, &d)?;
    }

    // 对应表、资源清单、给 AI 的盘点摘要
    let map = mx::build_url_map(&inv, &plan, &files);
    let mig = code.join(".pilot/migrate");
    write_json(&mig.join("urls.json"), &map)?;
    let ok_assets: Vec<String> = assets.values().filter(|l| public.join(mx::pct_decode(l.trim_start_matches('/'))).is_file()).cloned().collect();
    write_json(&mig.join("assets.json"), &ok_assets)?;
    std::fs::write(mig.join("survey.md"), survey_md(&inv, &plan, &map, &stats)).map_err(|e| e.to_string())?;
    job.step("files", "ok", &format!("对应表 {} 条", map.entries.len()));

    job.step("commit", "running", "");
    let _ = crate::site_git::git_ok(&code, &["add", "-A"], "").await;
    let msg = format!("迁移：导入旧站内容（{} 篇、{} 个图片和附件）", stats.pages, stats.assets);
    let _ = crate::site_git::git_commit_ok(&code, &["commit", "-qm", &msg]).await;
    job.step("commit", "ok", "");
    save_meta(&dir, |m| {
        m.extracted_at = now();
        m.extract = stats;
        m.parity_at = 0;
        m.parity.clear();
        m.live = LiveCheck::default();
    })?;
    Ok(())
}

async fn fetch_follow_bytes(c: &reqwest::Client, url: &str) -> Result<(Url, Vec<u8>), String> {
    let mut u = Url::parse(url).map_err(|e| e.to_string())?;
    for _ in 0..6 {
        let r = c.get(u.as_str()).send().await.map_err(|e| e.to_string())?;
        let s = r.status().as_u16();
        if (300..400).contains(&s) {
            let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            u = u.join(&loc).map_err(|e| e.to_string())?;
            continue;
        }
        if s >= 400 {
            return Err(format!("{s}"));
        }
        let b = r.bytes().await.map_err(|e| e.to_string())?;
        return Ok((u, b.to_vec()));
    }
    Err("跳转次数太多".into())
}

/// 给 AI 看的盘点摘要（`.pilot/migrate/survey.md`）。
fn survey_md(inv: &Inventory, plan: &Plan, map: &mx::UrlMap, st: &ExtractStats) -> String {
    let mut s = format!("# 旧站盘点：{}\n\n", inv.domain);
    s.push_str(&format!("- 实际地址：{}\n- 路径结尾：{}\n", inv.origin, match inv.trailing_slash.as_str() {
        "always" => "都以 / 结尾",
        "never" => "都不以 / 结尾",
        _ => "有的以 / 结尾、有的不以 / 结尾（逐个照原样）",
    }));
    if !inv.generator.is_empty() {
        s.push_str(&format!("- 旧站程序：{}\n", inv.generator));
    }
    if !inv.analytics.is_empty() {
        s.push_str(&format!("- 统计 ID：{}（GA4 已由 Pilot 存进配置变量 PUBLIC_GA_ID，代码从它读）\n", inv.analytics.join("、")));
    }
    let keep = map.entries.iter().filter(|e| e.action == "keep").count();
    let redirect = map.entries.iter().filter(|e| e.action == "redirect").count();
    s.push_str(&format!("- 对应表：保留 {keep} 个地址，跳转 {redirect} 个\n- 内容文件：{} 篇（{}）\n- 列表页 / 首页：{} 个（按内容生成，不是 Markdown）\n\n", st.pages, st.collections.iter().map(|(k, v)| format!("{k} {v} 篇")).collect::<Vec<_>>().join("、"), st.lists));
    s.push_str("## 分组\n\n| 分组 | 页数 | 类型 | 示例 |\n|---|---|---|---|\n");
    for g in &inv.groups {
        if plan.drop_groups.contains(&g.key) {
            continue;
        }
        s.push_str(&format!("| `{}` | {} | {} | {} |\n", g.key, g.count, g.ptype, g.samples.iter().take(3).map(|x| format!("`{x}`")).collect::<Vec<_>>().join(" ")));
    }
    if !inv.features.is_empty() {
        s.push_str("\n## 动态功能\n\n");
        for f in &inv.features {
            let how = match f.key.as_str() {
                "search" => "改成 Pagefind 静态搜索".to_string(),
                "form" | "comments" if st.features.contains(&f.key) => "保留：接口见重做要求里的【动态功能】".to_string(),
                "form" | "comments" => "方案里选了去掉：页面上去掉入口".to_string(),
                _ => f.plan.clone(),
            };
            s.push_str(&format!("- {}（{} 页，如 `{}`）：{how}\n", f.label, f.pages, f.samples.first().cloned().unwrap_or_default()));
        }
        if st.comments > 0 {
            s.push_str(&format!("- 旧评论 {} 条已抓到 `src/data/comments.json`\n", st.comments));
        }
    }
    if !st.large.is_empty() {
        s.push_str("\n## 放在 R2 的大文件（超过 25 MB，地址不变，不要放进 public/）\n\n");
        for l in st.large.iter().take(100) {
            s.push_str(&format!("- `{l}`\n"));
        }
    }
    let lists: Vec<&mx::UrlEntry> = map.entries.iter().filter(|e| e.action == "keep" && matches!(e.kind.as_str(), "list" | "home")).collect();
    if !lists.is_empty() {
        s.push_str("\n## 列表页和首页（要按内容生成，路径不变）\n\n");
        for e in lists.iter().take(200) {
            s.push_str(&format!("- `{}` {}\n", e.old, e.title));
        }
    }
    let feeds: Vec<&mx::UrlEntry> = map.entries.iter().filter(|e| e.action == "keep" && e.kind == "feed").collect();
    if !feeds.is_empty() {
        s.push_str("\n## RSS / Atom（地址不变）\n\n");
        for e in feeds {
            s.push_str(&format!("- `{}`\n", e.old));
        }
    }
    if !st.low_coverage.is_empty() {
        s.push_str("\n## 正文可能没抓全的页面（对照旧站快照截图或线上页面补齐）\n\n");
        for l in st.low_coverage.iter().take(100) {
            s.push_str(&format!("- {l}\n"));
        }
    }
    s
}

// ───────────────────────── ④ 重做 ─────────────────────────

/// 重做对话的第一条消息。
#[tauri::command]
pub async fn migrate_rebuild_message(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    let plan: Plan = read_json(&dir.join("plan.json")).ok_or("先确认迁移方案")?;
    let meta = load_meta(&dir);
    if meta.extracted_at == 0 {
        return Err("先抓取旧站内容".into());
    }
    let again = meta.rebuild_at > 0;
    // 抓取早于这个功能的站点也补上
    seed_ga(&state, &id, &inv)?;
    save_meta(&dir, |m| m.rebuild_at = now())?;
    let mut inv = inv;
    // 正式域名改过：canonical、站点地址按新的写
    if !meta.formal.is_empty() {
        inv.origin = format!("https://{}/", meta.formal);
    }
    Ok(rebuild_message(&site.name, &inv, &plan, &meta, again))
}

fn rebuild_message(site_name: &str, inv: &Inventory, plan: &Plan, meta: &Meta, again: bool) -> String {
    let visual = if plan.visual == "fresh" {
        "全新视觉：不沿用旧站的样子，按定位重新定设计方向。品牌名称照旧。".to_string()
    } else {
        "保留品牌：沿用旧站的 logo、站点名称和主色（看 `.pilot/migrate/screens/` 里的旧站截图和 `public/` 里的 favicon），在这个基础上把设计升级到下面的视觉标准。".to_string()
    };
    let style = if plan.style.is_empty() { String::new() } else { format!("\n补充要求：{}", plan.style) };
    let slash = match inv.trailing_slash.as_str() {
        "always" => "旧站地址都以 / 结尾：astro.config 里 `trailingSlash: 'always'`、`build.format: 'directory'`（产物是 a/index.html）。",
        "never" => "旧站地址都不以 / 结尾：astro.config 里 `trailingSlash: 'never'`、`build.format: 'file'`（产物是 a.html）。",
        _ => "旧站地址有的以 / 结尾、有的不以 / 结尾：`build.format: 'preserve'`，按 urls.json 里每个地址原样生成（以 / 结尾的生成 a/index.html，不以 / 结尾的生成 a.html）。",
    };
    let head = if again {
        format!("继续迁移重做 {}（Astro）。先看现状（`git log`、`.pilot/site.md`），已经做好的不要推倒重写，把下面没做到的补齐。", inv.domain)
    } else {
        format!("请迁移重做旧站 {}：用 Astro 重做全站，保留原来的每个 URL 和正文，把视觉升级。", inv.domain)
    };
    let issues = if meta.parity.iter().any(|c| c.level != "ok") || !meta.live.bad.is_empty() {
        let mut s = String::from("\n\n【上次核对没通过的】这一轮一起处理：\n");
        for c in meta.parity.iter().filter(|c| c.level != "ok") {
            s.push_str(&format!("- {}：{}\n", c.label, c.detail));
            for i in c.items.iter().take(8) {
                s.push_str(&format!("  - {i}\n"));
            }
        }
        if !meta.live.bad.is_empty() {
            s.push_str(&format!("- 预览地址实测（{}）有 {} 个地址不对：\n", meta.live.base, meta.live.bad.len()));
            for i in meta.live.bad.iter().take(8) {
                s.push_str(&format!("  - {i}\n"));
            }
        }
        s
    } else {
        String::new()
    };
    format!(
        "{head}\n\n【站点】{name}\n\n\
【已经准备好的】Pilot 已经盘点并抓取了旧站，提交在仓库里：\n\
- `.pilot/migrate/survey.md`：盘点摘要（分组、列表页、RSS、动态功能、正文可能没抓全的页面），先读它。\n\
- `.pilot/migrate/urls.json`：URL 对应表。`action: keep` 的地址新站必须原样生成；`redirect` 的写成 301；`drop` 的不用管。\n\
- `src/content/<集合>/…md`：每篇旧内容一份 Markdown。开头元数据：title（正文标题）、seoTitle（旧页面的 <title>，原样用作页面标题）、description、path（旧地址，页面就生成在这个路径）、date、updated、author、tags、image、lang、schema（旧页面有的结构化数据类型）、type。\n\
- `public/`：旧站的图片和附件，已经放在原路径上（外站图片在 `/assets/ext/` 下），正文里引用的就是这些路径。\n\
- `.pilot/migrate/screens/`：旧站首页和几类页面的截图。\n\n\
【迁移的硬约束】\n\
1. URL 一个都不能变：路由用每篇内容的 `path` 字段生成（`getStaticPaths` 按 path 出页面），不要按文件名推；列表页、首页、分类页按 urls.json 里的地址生成。{slash}\n\
2. 迁移来的正文一字不改写：只换外观和结构；页面 <title> 用 seoTitle 原样，description 原样；`schema` 里列的结构化数据类型都要输出（可以多，不能少）。\n\
3. canonical 指向正式域名 https://{origin_host} 上的原地址（站点地址走配置变量，默认就是它）。\n\
4. 旧站在跳转的地址（urls.json 里 action=redirect）写进 `public/_redirects`：`旧路径 新路径 301`，每行一条。\n\
5. RSS / Atom 地址不变（survey.md 里列着）：用 @astrojs/rss 生成；地址不以 .xml 结尾的（比如 /feed/），生成到 feed.xml 再在 `_redirects` 里写 `/feed/ /feed.xml 200` 改写。\n\
6. sitemap 要包含所有保留的页面（@astrojs/sitemap 即可）。\n\
7. 动态功能：站内搜索改成 Pagefind 静态索引；survey.md 里写「去掉」的功能，在页面上去掉入口；保留的按下面的【动态功能】接。\n\
8. survey.md 里列出「正文可能没抓全」的页面：对照旧站线上页面（可以用截图工具截）把缺的正文补上，补的内容必须来自旧页面本身。\n\n\
【视觉】{visual}{style}\n\n\
【做完之前】\n\
- 写站点约定 `.pilot/site.md`（技术栈、内容目录和元数据字段、新增一页要改哪里、构建和预览命令、设计方向），以后托管运营按它加内容；\n\
- 安装依赖、构建通过；本地预览，在 375、768、1280 宽度截图首页、一篇内容页、一个列表页，按视觉标准逐项评估、改完复查；\n\
- 自己抽查：urls.json 里随便挑 20 个 keep 地址，确认构建产物里都有对应文件（结尾斜杠也对）；\n\
- 需要我处理的事写进 `.pilot/todo.json`，`git add -A && git commit -m \"迁移重做\"`，简要汇报。{issues}\n\n\
【迁移里已经定好的，不用问我】\n\
- 正式域名就是 {origin_host}：切换那一步由 Pilot 把域名指过来，不要把正式域名写进待办（没有 `domain` 这一条）。\n\
- {ga}\n\
- 旧站页面上已经有的运营信息（邀请链接 / 邀请码、联系邮箱、社交账号……）照旧沿用：作为配置变量的默认值，并写进 `.env.example` 的示例。只有旧站上没有、或者明显已经失效的，才写进待办。\n\n\
{dynamic}{visual_std}\n\n{config}\n\n{todo}\n\n\
不要部署——上线和核对由 Pilot 负责。",
        name = site_name,
        origin_host = Url::parse(&inv.origin).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or(inv.domain.clone()),
        dynamic = if meta.extract.features.is_empty() {
            String::new()
        } else {
            let d = crate::site_dynamic::Dynamic { features: meta.extract.features.clone(), large: meta.extract.large.clone(), ..Default::default() };
            format!("{}\n\n", crate::site_dynamic::ai_contract(&d, meta.extract.comments > 0, plan.deploy == "vps"))
        },
        ga = match inv.analytics.iter().find(|a| a.starts_with("G-")) {
            Some(g) => format!("统计：旧站的 GA4（{g}）Pilot 已经存成配置变量 `PUBLIC_GA_ID`，代码按【配置变量】从它读就行。"),
            None => "统计：旧站没有 GA4，`PUBLIC_GA_ID` 留空即可，Pilot 会在看板上帮我创建。".into(),
        },
        visual_std = crate::site_make::VISUAL_STANDARD,
        config = crate::site_make::CONFIG_SPEC,
        todo = crate::site_make::TODO_SPEC,
    )
}

// ───────────────────────── ⑤ 核对 ─────────────────────────

const VERIFY_STEPS: &[(&str, &str)] = &[("install", "安装依赖"), ("build", "构建"), ("check", "检查"), ("parity", "对照旧站"), ("live", "预览地址实测")];

#[tauri::command]
pub async fn migrate_verify(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    if load_meta(&dir).extracted_at == 0 {
        return Err("先抓取旧站内容".into());
    }
    seed_ga(&state, &id, &inv)?;
    // seed_ga 可能刚改了配置变量的键：之后再取站点
    let site = state.sites.get(&id)?;
    let job = state.site_jobs.start(Some(app), &id, "migrate:verify", "核对新站", VERIFY_STEPS)?;
    let job_id = job.id.clone();
    let conns = state.conns.clone();
    tauri::async_runtime::spawn(async move {
        let r = run_verify(site, inv, dir, conns, &job).await;
        job.finish(&r);
    });
    Ok(job_id)
}

async fn run_verify(site: Site, inv: Inventory, dir: PathBuf, conns: crate::conns::ConnStore, job: &Job) -> Result<(), String> {
    let code = PathBuf::from(&site.local_dir);
    // Functions 代码由 Pilot 维护：构建前恢复成标准版本（AI 改过也以这份为准）
    let site_dir = dir.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.clone());
    let dynamic = crate::site_dynamic::load(&site_dir).filter(|d| !d.features.is_empty());
    if let Some(d) = &dynamic {
        crate::site_dynamic::write_code(&site.app_dir(&code), d)?;
    }
    let built = crate::site_deploy::build_and_check(&site, &code, None, job).await;
    let built = match built {
        Ok(b) => b,
        Err(e) => {
            save_meta(&dir, |m| {
                m.parity_at = now();
                m.parity = vec![mx::ParityCheck { key: "built".into(), label: "构建".into(), level: "block".into(), detail: e.clone(), items: Vec::new() }];
            })?;
            return Err(e);
        }
    };
    job.step("parity", "running", "");
    let map: mx::UrlMap = read_json(&code.join(".pilot/migrate/urls.json")).ok_or("仓库里没有 .pilot/migrate/urls.json")?;
    let assets: Vec<String> = read_json(&code.join(".pilot/migrate/assets.json")).unwrap_or_default();
    let out = built.out.clone();
    let refs = dir.join("ref");
    let (map2, inv2) = (map.clone(), inv.clone());
    let in_r2: Vec<String> = dynamic.as_ref().map(|d| d.large.clone()).unwrap_or_default();
    let routes_ok = dynamic.is_none() || built.out.join("_routes.json").is_file();
    let mut checks = tokio::task::spawn_blocking(move || mx::parity(&out, &map2, &inv2, &refs, &assets, &in_r2)).await.map_err(|e| e.to_string())?;
    if let Some(c) = crate::site_make::paper_background(&built.out) {
        checks.push(mx::ParityCheck {
            key: "paper_bg".into(),
            label: "页面底色".into(),
            level: "warn".into(),
            detail: format!("底色是米黄 / 奶油色（{c}）：{}", crate::site_make::PAPER_HINT),
            items: Vec::new(),
        });
    }
    if !routes_ok {
        checks.push(mx::ParityCheck {
            key: "routes".into(),
            label: "动态功能路由".into(),
            level: "block".into(),
            detail: "构建产物里没有 _routes.json（应该从 public/_routes.json 复制过去），表单、评论的接口会不通".into(),
            items: Vec::new(),
        });
    }
    let blocks = checks.iter().filter(|c| c.level == "block").count();
    let warns = checks.iter().filter(|c| c.level == "warn").count();
    save_meta(&dir, |m| {
        m.parity_at = now();
        m.parity = checks;
    })?;
    job.step("parity", if blocks > 0 { "failed" } else { "ok" }, &format!("{blocks} 项必须修，{warns} 项提醒"));

    let bad = run_live(&site, &dir, &map, dynamic.as_ref(), &conns, job).await?;
    match bad {
        Some(bad) if blocks > 0 || bad > 0 => Err(format!("核对没通过：{blocks} 项必须修，{}上 {bad} 个地址不对", if crate::site_vps_host::managed(&site) { "服务器" } else { "预览地址" })),
        _ if blocks > 0 => Err(format!("{blocks} 项必须修")),
        _ => Ok(()),
    }
}

/// 预览地址实测（作业里的 `live` 这一步）：Cloudflare 在 pages.dev 上测，自己的服务器带 Host 头在服务器上测。
/// 返回不对的地址数；还没上线过就是 None（这一步跳过）。
async fn run_live(site: &Site, dir: &Path, map: &mx::UrlMap, dynamic: Option<&crate::site_dynamic::Dynamic>, conns: &crate::conns::ConnStore, job: &Job) -> Result<Option<usize>, String> {
    let site_dir = dir.parent().map(Path::to_path_buf).unwrap_or_default();
    // 要先上线到 pages.dev（站点还没填正式域名，上线不会碰线上域名）
    let deployed = crate::site_deploy::load_history(&site_dir).iter().any(|r| r.ok);
    // 自己的服务器：在服务器上带 Host 头自测
    if crate::site_vps_host::managed(site) {
        let v = crate::site_vps_host::load(&site_dir).filter(|v| v.setup_at > 0 && !v.current.is_empty());
        let Some(v) = v.filter(|_| deployed) else {
            job.step("live", "skipped", "还没发布到服务器：配好服务器后点「发布到服务器」");
            save_meta(dir, |m| m.live = LiveCheck::default())?;
            return Ok(None);
        };
        job.step("live", "running", &format!("在服务器 {} 上自测", v.ip));
        let conn = conns.get(&site.deploy.ssh_conn_id).map_err(|_| "部署用的 SSH 连接不存在".to_string())?;
        let health = dynamic.is_some_and(|d| d.needs_db());
        let live = vps_check(&conn, &v, map, health).await?;
        let bad = live.bad.len();
        save_meta(dir, |m| m.live = live)?;
        job.step("live", if bad > 0 { "failed" } else { "ok" }, &if bad > 0 { format!("{bad} 个地址不对") } else { "旧地址都能正常打开或跳转".into() });
        return Ok(Some(bad));
    }
    if site.deploy.mode != "direct" || site.deploy.project.is_empty() || !deployed {
        job.step("live", "skipped", "还没上线到预览地址：配好 Cloudflare 后点「上线到预览地址」");
        save_meta(dir, |m| m.live = LiveCheck::default())?;
        return Ok(None);
    }
    job.step("live", "running", "");
    let base = match conns.get(&site.deploy.cf_conn_id) {
        Ok(cf) => match crate::keychain::get_key(&cf.id) {
            Ok(t) => crate::cf::pages_subdomain(&t, &cf.account_id, &site.deploy.project).await.map(|s| format!("https://{s}")).unwrap_or_else(|_| format!("https://{}.pages.dev", site.deploy.project)),
            Err(_) => format!("https://{}.pages.dev", site.deploy.project),
        },
        Err(_) => format!("https://{}.pages.dev", site.deploy.project),
    };
    let health = dynamic.is_some_and(|d| d.needs_db() || d.needs_r2());
    if health && dynamic.is_some_and(|d| d.setup_at == 0) {
        job.step("live", "failed", "动态功能还没配置：先点「配置动态功能」建数据库，再上线到预览地址");
        save_meta(dir, |m| m.live = LiveCheck { at: now(), base: base.clone(), checked: 0, bad: vec!["动态功能还没配置（数据库、人机验证），表单和评论的接口不通".into()] })?;
        return Err("动态功能还没配置".into());
    }
    let live = live_check(&base, map, health, job).await;
    let bad = live.bad.len();
    save_meta(dir, |m| m.live = live)?;
    job.step("live", if bad > 0 { "failed" } else { "ok" }, &if bad > 0 { format!("{bad} 个地址不对") } else { "旧地址都能正常打开或跳转".into() });
    Ok(Some(bad))
}

const LIVE_STEPS: &[(&str, &str)] = &[("live", "预览地址实测")];

/// 迁移中的站点上线 / 发布成功以后自动实测，不用再点「重新核对」（那会重新安装、构建一遍）。
/// 还没核对过（没有对应表）就不测。
pub async fn live_after_deploy(app: tauri::AppHandle, id: String) {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let ready = state.sites.get(&id).is_ok_and(|s| s.origin == "migrating") && load_meta(&mdir(&state, &id)).parity_at > 0;
    if ready {
        if let Ok(job) = start_live(&app, &state, &id) {
            run_live_job(app.clone(), id, job).await;
        }
    }
}

/// 只做「预览地址实测」这一步（已经上线过、不用重新构建）。
#[tauri::command]
pub async fn migrate_live(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let job = start_live(&app, &state, &id)?;
    let job_id = job.id.clone();
    tauri::async_runtime::spawn(run_live_job(app, id, job));
    Ok(job_id)
}

fn start_live(app: &tauri::AppHandle, state: &AppState, id: &str) -> Result<Job, String> {
    state.sites.get(id)?;
    state.site_jobs.start(Some(app.clone()), id, "migrate:live", "预览地址实测", LIVE_STEPS)
}

async fn run_live_job(app: tauri::AppHandle, id: String, job: Job) {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let r = async {
        let site = state.sites.get(&id)?;
        let dir = mdir(&state, &id);
        let map: mx::UrlMap = read_json(&PathBuf::from(&site.local_dir).join(".pilot/migrate/urls.json")).ok_or("仓库里没有 .pilot/migrate/urls.json")?;
        let site_dir = dir.parent().map(Path::to_path_buf).unwrap_or_default();
        let dynamic = crate::site_dynamic::load(&site_dir).filter(|d| !d.features.is_empty());
        match run_live(&site, &dir, &map, dynamic.as_ref(), &state.conns, &job).await? {
            Some(bad) if bad > 0 => Err(format!("{bad} 个地址不对")),
            Some(_) => Ok(()),
            None => Err("还没上线过".to_string()),
        }
    }
    .await;
    job.finish(&r);
}

/// 在某个地址上把对应表里的旧路径都请求一遍：keep 要 200，redirect 要 301/308 到新地址。
/// 一个旧地址的实测结果对不对：keep 要 200，redirect 要 301 / 308 到新地址。
fn judge(e: &mx::UrlEntry, s: u16, loc: &str) -> Option<String> {
    let loc_path = Url::parse(loc).map(|u| sm::path_of(&u)).unwrap_or_else(|_| loc.to_string());
    match e.action.as_str() {
        "redirect" if matches!(s, 301 | 308) && (loc.ends_with(&e.new) || loc == e.new || loc_path == e.new) => None,
        "redirect" => Some(format!("{}：应该 301 到 {}，实际 {s} {loc}", e.old, e.new)),
        _ if s == 200 => None,
        _ if matches!(s, 301 | 308) => Some(format!("{}：多跳了一次到 {loc}（结尾斜杠或 .html 不一致）", e.old)),
        _ => Some(format!("{}：返回 {s}", e.old)),
    }
}

/// 自己的服务器：域名还没指过来，在服务器上带 Host 头把每个旧路径请求一遍。
async fn vps_check(conn: &crate::conns::Connection, v: &crate::site_vps_host::VpsState, map: &mx::UrlMap, health: bool) -> Result<LiveCheck, String> {
    let entries: Vec<&mx::UrlEntry> = map.entries.iter().filter(|e| e.action != "drop").collect();
    let mut paths: Vec<String> = entries.iter().map(|e| e.old.clone()).collect();
    if health {
        paths.push("/api/health".into());
    }
    let got = crate::site_vps_host::remote_check(conn, v, &paths).await?;
    let mut out = LiveCheck { at: now(), base: format!("服务器 {}（{}）", v.ip, v.host), checked: got.len(), bad: Vec::new() };
    let by: std::collections::HashMap<&str, (u16, &str)> = got.iter().map(|(p, c, l)| (p.as_str(), (*c, l.as_str()))).collect();
    if health && by.get("/api/health").is_none_or(|(c, _)| *c != 200) {
        out.bad.push(format!("/api/health：返回 {}（表单和评论服务没在跑）", by.get("/api/health").map(|x| x.0).unwrap_or(0)));
    }
    for e in entries {
        match by.get(e.old.as_str()) {
            Some((c, l)) => {
                if let Some(b) = judge(e, *c, l) {
                    out.bad.push(b);
                }
            }
            None => out.bad.push(format!("{}：没测到", e.old)),
        }
    }
    out.bad.sort();
    Ok(out)
}

async fn live_check(base: &str, map: &mx::UrlMap, health: bool, job: &Job) -> LiveCheck {
    let c = client();
    let mut out = LiveCheck { at: now(), base: base.to_string(), ..Default::default() };
    // 动态功能：/api/health 要 200 且数据库连得上
    if health {
        let url = format!("{}/api/health", base.trim_end_matches('/'));
        match fetch(&c, &url, false).await {
            Ok(f) if f.status == 200 && f.body.contains("\"db\":true") => {}
            Ok(f) => out.bad.push(format!("/api/health：返回 {}（动态功能没生效：配置后要再上线一次）", f.status)),
            Err(e) => out.bad.push(format!("/api/health：打不开（{e}）")),
        }
        out.checked += 1;
    }
    let entries: Vec<mx::UrlEntry> = map.entries.iter().filter(|e| e.action != "drop").cloned().collect();
    let total = entries.len();
    for chunk in entries.chunks(6) {
        if job.canceled() {
            break;
        }
        let mut set = tokio::task::JoinSet::new();
        for e in chunk {
            let (c, url, e) = (c.clone(), format!("{}{}", base.trim_end_matches('/'), e.old), e.clone());
            set.spawn(async move {
                let r = match c.get(&url).send().await {
                    Ok(r) => r,
                    Err(err) => return Some(format!("{}：打不开（{err}）", e.old)),
                };
                let s = r.status().as_u16();
                let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                judge(&e, s, &loc)
            });
        }
        while let Some(r) = set.join_next().await {
            if let Ok(Some(b)) = r {
                out.bad.push(b);
            }
        }
        out.checked += chunk.len();
        job.step("live", "running", &format!("{} / {total}", out.checked));
    }
    out.bad.sort();
    out
}

// ───────────────────────── ⑥ 切换 ─────────────────────────

/// 所有权：Cloudflare 账号里有这个域名且已生效、Search Console 已验证、或者 DNS 里加了 TXT 记录。
#[tauri::command]
pub async fn migrate_ownership(state: tauri::State<'_, AppState>, id: String) -> Result<sm::Ownership, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let mut inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    let domain = site.migrate_from.clone();
    let mut own = inv.ownership.clone();
    own.verified = false;
    own.by.clear();
    for cf in state.conns.list().into_iter().filter(|c| c.kind == "cloudflare") {
        let Ok(token) = crate::keychain::get_key(&cf.id) else { continue };
        if let Ok(Some(z)) = crate::cf::zone_for(&token, &domain).await {
            own.cf_conn = cf.id.clone();
            own.cf_zone = z.id.clone();
            if crate::cf::zone_info(&token, &z.id).await.map(|x| x.0 == "active").unwrap_or(false) {
                own.verified = true;
                own.by = "cloudflare".into();
            }
            break;
        }
    }
    if !own.verified && !site.data.gsc_site.is_empty() {
        own.gsc_site = site.data.gsc_site.clone();
        own.verified = true;
        own.by = "gsc".into();
    }
    // 站点还没绑 Search Console：看看连接的 Google 账号里有没有已验证这个站（有完整或所有者权限才算）
    if !own.verified {
        for c in state.conns.list().into_iter().filter(|c| c.kind == "google") {
            let Ok(raw) = crate::keychain::get_key(&c.id) else { continue };
            let Ok(cred) = crate::google::parse_credential(&c.key_kind, &raw) else { continue };
            let Ok(t) = crate::google::access_token(&c.id, &cred, &crate::google::read_scopes()).await else { continue };
            let Ok(list) = crate::google::list_gsc_sites(&t).await else { continue };
            if let Some(g) = crate::google::match_gsc_site(&list, &domain).filter(|g| matches!(g.permission.as_str(), "siteOwner" | "siteFullUser")) {
                own.gsc_site = g.url.clone();
                own.verified = true;
                own.by = "gsc".into();
                break;
            }
        }
    }
    if !own.verified {
        let apex = apex_of(&domain);
        let txt = doh(&apex, "TXT").await;
        if txt.iter().any(|r| r.content.contains(&own.txt_token)) {
            own.verified = true;
            own.by = "txt".into();
        }
    }
    inv.ownership = own.clone();
    write_json(&dir.join("inventory.json"), &inv)?;
    Ok(own)
}

/// 公开 DNS 记录快照：根域、www、常见子域名、邮件相关的记录。
async fn dns_snapshot(apex: &str, extra_hosts: &[String]) -> Vec<FullRec> {
    let mut names: Vec<String> = vec![apex.to_string(), format!("www.{apex}")];
    for s in ["mail", "smtp", "imap", "pop", "webmail", "autodiscover", "autoconfig", "blog", "shop", "m", "api", "cdn", "static", "img", "ftp", "_dmarc", "default._domainkey", "google._domainkey", "selector1._domainkey", "selector2._domainkey", "k1._domainkey", "s1._domainkey", "s2._domainkey", "dkim._domainkey", "mx._domainkey"] {
        names.push(format!("{s}.{apex}"));
    }
    for h in extra_hosts {
        if h.ends_with(&format!(".{apex}")) && !names.contains(h) {
            names.push(h.clone());
        }
    }
    // 每个名字一个任务并发查（顺着查一百多次要二十来秒），结果按原来的顺序拼回去
    let mut set = tokio::task::JoinSet::new();
    for (i, n) in names.into_iter().enumerate() {
        let is_root = n == apex;
        set.spawn(async move {
            let kinds: &[&str] = if is_root { &["A", "AAAA", "MX", "TXT", "CAA"] } else { &["CNAME", "A", "AAAA", "MX", "TXT"] };
            let mut out = Vec::new();
            for k in kinds {
                let recs = doh(&n, k).await;
                let cname = *k == "CNAME" && !recs.is_empty();
                out.extend(recs);
                if cname {
                    break;
                }
            }
            (i, out)
        });
    }
    let mut got = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok(x) = r {
            got.push(x);
        }
    }
    got.sort_by_key(|x| x.0);
    got.into_iter().flat_map(|x| x.1).collect()
}

fn same_rec(a: &FullRec, b: &FullRec) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches('.').trim_matches('"').to_ascii_lowercase();
    a.kind == b.kind && norm(&a.name) == norm(&b.name) && norm(&a.content) == norm(&b.content)
}

/// 准备把域名接进 Cloudflare：存一份现在的 DNS 记录；不在 Cloudflare 里就用站点的 Cloudflare 连接建一个 Zone，
/// 对照 Cloudflare 自动导入的记录，列出缺的（尤其是邮件记录）。
#[tauri::command]
pub async fn migrate_dns_prepare(state: tauri::State<'_, AppState>, id: String, create: Option<bool>) -> Result<DnsPrep, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    let meta = load_meta(&dir);
    let apex = apex_of(&formal_host(&inv, &site, &meta));
    let mut prep = meta.dns;
    // 正式域名换了：之前那份快照是别的域名的
    if !prep.snapshot.is_empty() && prep.snapshot.iter().all(|r| !r.name.eq_ignore_ascii_case(&apex) && !r.name.to_ascii_lowercase().ends_with(&format!(".{apex}"))) {
        prep = DnsPrep::default();
    }
    if prep.snapshot.is_empty() {
        let hosts: Vec<String> = inv.assets.keys().filter_map(|u| Url::parse(u).ok().and_then(|u| u.host_str().map(String::from))).collect::<HashSet<_>>().into_iter().collect();
        prep.snapshot = dns_snapshot(&apex, &hosts).await;
        prep.at = now();
    }
    prep.current_ns = doh(&apex, "NS").await.into_iter().map(|r| r.content).collect();
    // 找 Zone：先在所有 Cloudflare 连接里找，没有再按需新建（用站点部署用的那条连接）
    let mut found = None;
    for cf in state.conns.list().into_iter().filter(|c| c.kind == "cloudflare") {
        let Ok(token) = crate::keychain::get_key(&cf.id) else { continue };
        if let Ok(Some(z)) = crate::cf::zone_for(&token, &apex).await {
            found = Some((cf, token, z));
            break;
        }
    }
    if found.is_none() && create.unwrap_or(false) {
        let cf = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "先在「上线」里选好 Cloudflare 连接".to_string())?;
        let token = crate::keychain::get_key(&cf.id)?;
        let (z, ns) = crate::cf::create_zone(&token, &cf.account_id, &apex).await.map_err(|e| format!("把 {apex} 加进 Cloudflare 失败：{e}（令牌需要「区域 → 区域 → 编辑」权限）"))?;
        prep.created_zone = true;
        prep.ns = ns;
        // Cloudflare 扫描现有记录要几秒
        tokio::time::sleep(Duration::from_secs(6)).await;
        found = Some((cf, token, z));
    }
    if let Some((cf, token, z)) = found {
        prep.cf_conn = cf.id.clone();
        prep.zone_id = z.id.clone();
        let (status, ns) = crate::cf::zone_info(&token, &z.id).await.unwrap_or_default();
        prep.zone_status = status;
        if !ns.is_empty() {
            prep.ns = ns;
        }
        // 已经在 Cloudflare 里生效的域名，Cloudflare 就是现在的 DNS，没有「缺」的说法
        // （公开查到的代理记录是 Cloudflare 的 IP，拿来对照反而全是误报）
        prep.missing = if prep.zone_status == "active" {
            Vec::new()
        } else {
            let have = crate::cf::all_dns_records(&token, &z.id).await.unwrap_or_default();
            prep.snapshot.iter().filter(|r| !have.iter().any(|h| same_rec(r, h))).filter(|r| r.kind != "NS").cloned().collect()
        };
    }
    save_meta(&dir, |m| m.dns = prep.clone())?;
    Ok(prep)
}

/// 把快照里有、Cloudflare 里没有的记录补上（界面上确认过才调用）。
#[tauri::command]
pub async fn migrate_dns_fix(state: tauri::State<'_, AppState>, id: String) -> Result<DnsPrep, String> {
    let dir = mdir(&state, &id);
    let prep = load_meta(&dir).dns;
    if prep.zone_id.is_empty() {
        return Err("域名还没接进 Cloudflare".into());
    }
    let cf = state.conns.get(&prep.cf_conn)?;
    let token = crate::keychain::get_key(&cf.id)?;
    let mut errs = Vec::new();
    for r in &prep.missing {
        // 不确定要不要代理：A / AAAA / CNAME 先不代理，和原来的行为一致
        if let Err(e) = crate::cf::create_dns_record(&token, &prep.zone_id, &FullRec { proxied: false, ..r.clone() }).await {
            errs.push(format!("{} {}：{e}", r.kind, r.name));
        }
    }
    let p = migrate_dns_prepare(state, id, Some(false)).await?;
    if errs.is_empty() {
        Ok(p)
    } else {
        Err(format!("有 {} 条没补上：{}", errs.len(), errs.join("；")))
    }
}

/// 切换：把正式域名指向新站。要求核对通过、所有权已确认、域名的 DNS 已经在 Cloudflare 里生效。
#[tauri::command]
pub async fn migrate_cutover(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<Cutover, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    let plan: Option<Plan> = read_json(&dir.join("plan.json"));
    let meta = load_meta(&dir);
    if stage(Some(&inv), plan.as_ref(), &meta) != "cutover" {
        return Err("核对还没全部通过（包括预览地址实测），先处理完再正式上线".into());
    }
    let host = formal_host(&inv, &site, &meta);
    // 上线到旧站的域名：要先确认旧站是你的。换了新域名：新域名在你的 Cloudflare 账号里就算确认（下面 zone_for 会查）
    if host == origin_host(&inv, &site) && !inv.ownership.verified {
        return Err("还没确认这个站是你的：先完成「确认这个站是你的」".into());
    }
    if crate::site_vps_host::managed(&site) {
        return cutover_vps(&state, &site, &dir, &host).await;
    }
    let cf = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "站点的 Cloudflare 连接不存在".to_string())?;
    let token = crate::keychain::get_key(&cf.id)?;
    let zone = crate::cf::zone_for(&token, &host).await?.ok_or_else(|| format!("{host} 的 DNS 不在这个 Cloudflare 账号里：先在「正式上线」里把域名接进 Cloudflare"))?;
    let (status, _) = crate::cf::zone_info(&token, &zone.id).await?;
    if status != "active" {
        return Err(format!("{} 在 Cloudflare 里还没生效（{status}）：去域名注册商把名称服务器改成 Cloudflare 给的那两个，生效后再正式上线", zone.name));
    }
    let target = crate::cf::pages_subdomain(&token, &cf.account_id, &site.deploy.project).await?;
    // 存下这个主机名原来的记录（回退用）
    let before: Vec<FullRec> = crate::cf::all_dns_records(&token, &zone.id)
        .await?
        .into_iter()
        .filter(|r| r.name.eq_ignore_ascii_case(&host) && matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME"))
        .collect();
    let mut cut = Cutover { at: now(), host: host.clone(), zone_id: zone.id.clone(), before: before.clone(), notes: Vec::new(), rolled_back_at: 0, cf_conn: cf.id.clone(), manual_dns: false };
    save_meta(&dir, |m| m.cutover = Cutover { at: 0, ..cut.clone() })?;
    crate::cf::add_pages_domain(&token, &cf.account_id, &site.deploy.project, &host).await?;
    for r in &before {
        crate::cf::delete_dns_record(&token, &zone.id, &r.id).await?;
    }
    crate::cf::ensure_dns_record(&token, &zone.id, "CNAME", &host, &target, true).await?;
    let _ = crate::cf::purge_hosts(&token, &zone.id, std::slice::from_ref(&host)).await;
    // 站点正式填上域名，移出「制作中」
    let site = state.sites.update(&id, now(), |s| {
        s.domain = host.clone();
        s.origin = "migrated".into();
        Ok(())
    })?;
    let old_host = origin_host(&inv, &site);
    if host != old_host {
        // 换了域名：站点地址变量（AI 按【配置变量】写的）改成新域名，canonical / sitemap 跟着变
        let code = PathBuf::from(&site.local_dir);
        let declared = std::fs::read_to_string(site.app_dir(&code).join(".env.example")).unwrap_or_default();
        if let Some(key) = ["PUBLIC_SITE_URL", "SITE_URL", "PUBLIC_SITE", "SITE"].iter().find(|k| declared.lines().any(|l| l.trim_start().starts_with(&format!("{k}=")))) {
            let keys = crate::sites::save_env(&id, &[(key.to_string(), format!("https://{host}"))], &[])?;
            state.sites.update(&id, now(), |s| {
                s.build.env_keys = keys;
                Ok(())
            })?;
        }
    }
    cut.notes = direct_notes(&state, &site, &inv, &host, &load_meta(&dir));
    save_meta(&dir, |m| m.cutover = cut.clone())?;
    // 生效检查 + 补上 IndexNow 验证文件：上线一次（这次带着正式域名）
    let _ = crate::site_cmds::start_deploy(app, &state, site.id.clone());
    Ok(cut)
}

/// 上线到 Cloudflare 以后给你看的几句话（按现在的情况算，界面每次打开都重新算）。
fn direct_notes(state: &AppState, site: &Site, inv: &Inventory, host: &str, m: &Meta) -> Vec<String> {
    let old = origin_host(inv, site);
    let mut n = vec![format!("{host} 已经打开新站，几分钟内全部生效")];
    // 只有根域名 / www 才有「另一种写法」
    let apex = apex_of(host);
    if host == apex || host == format!("www.{apex}") {
        let other = if host == apex { format!("www.{apex}") } else { apex.clone() };
        n.push(format!("另一种写法 {other} 没有改动，原来打开什么现在还是什么"));
    }
    if host == old {
        n.push("旧站的服务器先别关：新站稳定两周左右再停".into());
    } else {
        let env = crate::sites::load_env(&site.id);
        let url = format!("https://{host}");
        if env.iter().any(|(k, v)| ["PUBLIC_SITE_URL", "SITE_URL", "PUBLIC_SITE", "SITE"].contains(&k.as_str()) && v.trim_end_matches('/') == url) {
            n.push(format!("站点里的网址已经改成 {url}（给搜索引擎看的地址、网站地图都跟着改了）"));
        } else {
            n.push(format!("站点里的网址可能还写着 {old}：让 AI 改成 {host}"));
        }
        n.push(if m.old_redirect.at > 0 {
            format!("旧域名 {old} 的每个地址都会跳到 {host} 的同一地址，原来的搜索排名会慢慢转过来")
        } else if inv.ownership.verified {
            format!("旧域名 {old} 还在打开旧站。等新站稳定了，让它跳转到 {host}，原来的搜索排名才能带过来")
        } else {
            format!("旧站 {old} 不受影响；搜索排名要在 {host} 上重新积累")
        });
    }
    if crate::site_dynamic::load(&state.sites.data_dir(&site.id)).is_some_and(|d| d.setup_at > 0 && d.needs_db()) {
        n.push("表单和评论照常可用，收到的提交和评论在概览的「表单和评论」里看".into());
    }
    n
}

/// 回退：把这个主机名的记录恢复成切换前的样子，站点回到迁移中。
#[tauri::command]
pub async fn migrate_rollback(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let meta = load_meta(&dir);
    let cut = meta.cutover.clone();
    if crate::site_vps_host::managed(&site) && !cut.host.is_empty() {
        return rollback_vps(&state, &site, &dir, &cut).await;
    }
    if cut.host.is_empty() || cut.zone_id.is_empty() {
        return Err("还没正式上线过".into());
    }
    // 旧域名正跳到新站：先撤掉，不然退回以后旧域名会跳到一个不再打开新站的地址
    if meta.old_redirect.at > 0 {
        undo_old_redirect(&state, &dir, &meta.old_redirect).await?;
    }
    let cf = state.conns.get(&site.deploy.cf_conn_id)?;
    let token = crate::keychain::get_key(&cf.id)?;
    let now_recs = crate::cf::all_dns_records(&token, &cut.zone_id).await?;
    for r in now_recs.iter().filter(|r| r.name.eq_ignore_ascii_case(&cut.host) && matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME")) {
        crate::cf::delete_dns_record(&token, &cut.zone_id, &r.id).await?;
    }
    for r in &cut.before {
        crate::cf::create_dns_record(&token, &cut.zone_id, r).await?;
    }
    let _ = crate::cf::delete_pages_domain(&token, &cf.account_id, &site.deploy.project, &cut.host).await;
    let _ = crate::cf::purge_hosts(&token, &cut.zone_id, std::slice::from_ref(&cut.host)).await;
    let host = cut.host.clone();
    state.sites.update(&id, now(), |s| {
        s.domain.clear();
        s.origin = "migrating".into();
        if s.migrate_from.is_empty() {
            s.migrate_from = host.clone();
        }
        Ok(())
    })?;
    save_meta(&dir, |m| {
        m.cutover.at = 0;
        m.cutover.rolled_back_at = now();
        m.track = Track::default();
    })?;
    Ok(format!("{} 已恢复成上线前的 {} 条记录，打开的又是旧站了（DNS 缓存可能要几分钟）", cut.host, cut.before.len()))
}

// ───────────────────────── ⑥ 切换前：正式域名 / 测试域名 ─────────────────────────

/// 改正式域名（空＝恢复成旧站的域名）。切换前才能改；自己的服务器暂不支持换域名。
#[tauri::command]
pub async fn migrate_set_formal(state: tauri::State<'_, AppState>, id: String, host: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    if load_meta(&dir).cutover.at > 0 {
        return Err("已经正式上线了：先退回旧站再改正式域名".into());
    }
    let host = if host.trim().is_empty() { String::new() } else { clean_host(&host)? };
    let old = origin_host(&inv, &site);
    let host = if host == old { String::new() } else { host };
    if !host.is_empty() && crate::site_vps_host::managed(&site) {
        return Err("部署在自己的服务器上的站点暂时不支持换域名".into());
    }
    save_meta(&dir, |m| {
        if m.formal != host {
            m.dns = DnsPrep::default();
        }
        m.formal = host.clone();
    })?;
    Ok(if host.is_empty() { old } else { host })
}

/// 一个域名现在有没有在用：指向哪、能不能打开、打开是什么。改正式域名、正式上线前给用户看。
#[derive(Serialize, Default)]
pub struct HostUse {
    pub host: String,
    /// 在已连接的 Cloudflare 账号里、已生效（Pilot 能自动改）
    pub in_cf: bool,
    /// 现有的指向（A / AAAA / CNAME），给人看的
    pub records: Vec<String>,
    /// 已经指向这个新站的 Pages 项目
    pub ours: bool,
    /// 指向别的 Pages 项目（项目名）
    pub other_pages: String,
    /// 打开它的结果：状态码（0＝打不开）和页面标题
    pub status: u16,
    pub title: String,
    /// 就是旧站的域名
    pub old_site: bool,
}

#[tauri::command]
pub async fn migrate_host_use(state: tauri::State<'_, AppState>, id: String, host: String) -> Result<HostUse, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    let host = clean_host(&host)?;
    let bare = |h: &str| h.trim_start_matches("www.").to_string();
    let mut u = HostUse { old_site: bare(&host) == bare(&origin_host(&inv, &site)), host: host.clone(), ..Default::default() };
    // Cloudflare 偶尔一次请求失败（网络、代理）：没找到就再查一次，别把「没查到」当成「不是你的」
    let mut found = cf_zone_for(&state, &host).await;
    if found.is_none() {
        tokio::time::sleep(Duration::from_millis(800)).await;
        found = cf_zone_for(&state, &host).await;
    }
    let recs: Vec<FullRec> = if let Some((_, token, zone)) = found {
        u.in_cf = true;
        crate::cf::all_dns_records(&token, &zone.id).await.unwrap_or_default().into_iter().filter(|r| r.name.eq_ignore_ascii_case(&host) && matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME")).collect()
    } else {
        let mut v = doh(&host, "CNAME").await;
        if v.is_empty() {
            v = doh(&host, "A").await;
        }
        v
    };
    let ours = format!("{}.pages.dev", site.deploy.project);
    for r in &recs {
        let c = r.content.trim_end_matches('.').to_ascii_lowercase();
        if r.kind == "CNAME" && !site.deploy.project.is_empty() && c == ours {
            u.ours = true;
        } else if r.kind == "CNAME" && c.ends_with(".pages.dev") {
            u.other_pages = c.trim_end_matches(".pages.dev").to_string();
        }
        u.records.push(format!("{} → {}{}", r.kind, c, if r.proxied { "（经 Cloudflare 代理）" } else { "" }));
    }
    if !recs.is_empty() {
        if let Ok(r) = reqwest::Client::builder().timeout(Duration::from_secs(8)).redirect(reqwest::redirect::Policy::limited(4)).build().map_err(|e| e.to_string())?.get(format!("https://{host}/")).send().await {
            u.status = r.status().as_u16();
            let body = r.text().await.unwrap_or_default();
            let low = body.to_ascii_lowercase();
            if let (Some(a), Some(b)) = (low.find("<title"), low.find("</title>")) {
                if let Some(gt) = body[a..].find('>') {
                    if a + gt + 1 <= b {
                        u.title = body[a + gt + 1..b].split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
                    }
                }
            }
        }
    }
    Ok(u)
}

/// 先上线到测试域名：绑到同一个 Pages 项目，加一条 CNAME。这个主机名已经有记录就不碰。
/// 页面不让收录：上线时 Pilot 给测试域名和 pages.dev 加 `X-Robots-Tag: noindex`（见 site_deploy::add_verify_files）。
#[tauri::command]
pub async fn migrate_staging(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String, host: String) -> Result<Staging, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    let meta = load_meta(&dir);
    if crate::site_vps_host::managed(&site) {
        return Err("部署在自己的服务器上的站点暂时不支持测试域名".into());
    }
    if site.deploy.mode != "direct" || site.deploy.project.is_empty() {
        return Err("先在核对里「准备预览地址」".into());
    }
    let host = clean_host(&host)?;
    let formal = formal_host(&inv, &site, &meta);
    let bare = |h: &str| h.trim_start_matches("www.").to_string();
    if bare(&host) == bare(&formal) || bare(&host) == bare(&origin_host(&inv, &site)) {
        return Err(format!("{host} 是正式域名本身：测试域名要用另一个，比如 new.{}", bare(&formal)));
    }
    if !meta.staging.host.is_empty() && meta.staging.host != host {
        return Err(format!("已经有测试域名 {}：先删掉再换", meta.staging.host));
    }
    let (cf, token, zone) = cf_zone_for(&state, &host).await.ok_or_else(|| format!("{host} 的 DNS 不在已连接的 Cloudflare 账号里（或者还没生效）：测试域名要用 Cloudflare 里已经生效的域名"))?;
    let have: Vec<FullRec> = crate::cf::all_dns_records(&token, &zone.id).await?.into_iter().filter(|r| r.name.eq_ignore_ascii_case(&host)).collect();
    let pages_cf = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "站点的 Cloudflare 连接不存在".to_string())?;
    let pages_token = crate::keychain::get_key(&pages_cf.id)?;
    let target = crate::cf::pages_subdomain(&pages_token, &pages_cf.account_id, &site.deploy.project).await?;
    if have.iter().any(|r| !(r.kind == "CNAME" && r.content.eq_ignore_ascii_case(&target))) {
        return Err(format!("{host} 已经有 DNS 记录（{}），为了不影响现有服务没有动它：换一个没在用的子域名", have.iter().map(|r| format!("{} {}", r.kind, r.content)).collect::<Vec<_>>().join("、")));
    }
    let st = Staging { host: host.clone(), at: now(), zone_id: zone.id.clone(), cf_conn: cf.id.clone(), status: "pending".into(), live: LiveCheck::default() };
    // 先记下来、再上线一次（带上 noindex），最后才把域名指过来：不会有一刻能被收录
    save_meta(&dir, |m| m.staging = st.clone())?;
    let job_id = crate::site_cmds::start_deploy(app.clone(), &state, id.clone())?;
    wait_job(&state, &id, &job_id).await?;
    crate::cf::add_pages_domain(&pages_token, &pages_cf.account_id, &site.deploy.project, &host).await?;
    crate::cf::ensure_dns_record(&token, &zone.id, "CNAME", &host, &target, true).await?;
    Ok(st)
}

/// 等一次上线结束（成功才算）。上线成功后会紧接着开始实测作业，那时就看部署记录。
async fn wait_job(state: &AppState, site_id: &str, job_id: &str) -> Result<(), String> {
    let started = now();
    for _ in 0..1800 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        match state.site_jobs.snapshot(site_id) {
            Some(j) if j.id == job_id && j.status == "running" => continue,
            Some(j) if j.id == job_id && j.status == "ok" => return Ok(()),
            Some(j) if j.id == job_id => return Err(format!("上线没成功：{}", j.error.lines().next().unwrap_or(""))),
            _ => {
                let last = crate::site_deploy::load_history(&state.sites.data_dir(site_id)).into_iter().next();
                return match last {
                    Some(r) if r.ok && r.ts >= started.saturating_sub(5) => Ok(()),
                    _ => Err("上线没成功".into()),
                };
            }
        }
    }
    Err("上线超时".into())
}

/// 删掉测试域名：解除 Pages 绑定、删掉那条 CNAME（只删指向这个 Pages 项目的）。
#[tauri::command]
pub async fn migrate_staging_remove(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let st = load_meta(&dir).staging;
    if st.host.is_empty() {
        return Err("没有测试域名".into());
    }
    if let Ok(pages_cf) = state.conns.get(&site.deploy.cf_conn_id) {
        if let Ok(t) = crate::keychain::get_key(&pages_cf.id) {
            let _ = crate::cf::delete_pages_domain(&t, &pages_cf.account_id, &site.deploy.project, &st.host).await;
        }
    }
    let cf = state.conns.get(&st.cf_conn)?;
    let token = crate::keychain::get_key(&cf.id)?;
    for r in crate::cf::all_dns_records(&token, &st.zone_id).await? {
        if r.name.eq_ignore_ascii_case(&st.host) && r.kind == "CNAME" && r.content.ends_with(".pages.dev") {
            crate::cf::delete_dns_record(&token, &st.zone_id, &r.id).await?;
        }
    }
    save_meta(&dir, |m| m.staging = Staging::default())?;
    Ok(format!("已删掉测试域名 {}", st.host))
}

/// 在测试域名上把每个旧地址打开一遍（只是给你看，不影响能不能切换）。
#[tauri::command]
pub async fn migrate_staging_check(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    start_staging_check(&app, &state, &id)
}

fn start_staging_check(app: &tauri::AppHandle, state: &AppState, id: &str) -> Result<String, String> {
    let site = state.sites.get(id)?;
    let dir = mdir(state, id);
    let st = load_meta(&dir).staging;
    if st.host.is_empty() {
        return Err("还没有测试域名".into());
    }
    let map: mx::UrlMap = read_json(&PathBuf::from(&site.local_dir).join(".pilot/migrate/urls.json")).ok_or("仓库里没有 .pilot/migrate/urls.json")?;
    let job = state.site_jobs.start(Some(app.clone()), id, "migrate:staging", "测试域名实测", LIVE_STEPS)?;
    let job_id = job.id.clone();
    tauri::async_runtime::spawn(async move {
        job.step("live", "running", &st.host);
        let live = live_check(&format!("https://{}", st.host), &map, false, &job).await;
        let bad = live.bad.len();
        let _ = save_meta(&dir, |m| m.staging.live = live);
        job.step("live", if bad > 0 { "failed" } else { "ok" }, &if bad > 0 { format!("{bad} 个地址不对") } else { "旧地址都能正常打开或跳转".into() });
        job.finish(&if bad > 0 { Err(format!("{bad} 个地址不对")) } else { Ok(()) });
    });
    Ok(job_id)
}

/// 测试域名生效了没有（界面在 pending 时每隔一会儿问一次）：生效了就自动实测一遍。
#[tauri::command]
pub async fn migrate_staging_status(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let st = load_meta(&dir).staging;
    if st.host.is_empty() {
        return Ok(String::new());
    }
    if st.status == "active" {
        return Ok(st.status);
    }
    let cf = state.conns.get(&site.deploy.cf_conn_id).map_err(|_| "站点的 Cloudflare 连接不存在".to_string())?;
    let token = crate::keychain::get_key(&cf.id)?;
    let status = crate::cf::pages_domain_status(&token, &cf.account_id, &site.deploy.project, &st.host).await?.unwrap_or_else(|| "pending".into());
    // Pages 说生效了还不够：真能打开才算（证书刚签好的那一会儿还会 522）
    let opens = status == "active" && reqwest::Client::new().head(format!("https://{}/", st.host)).timeout(Duration::from_secs(10)).send().await.is_ok_and(|r| r.status().as_u16() < 500);
    let status = if opens { "active".to_string() } else { "pending".to_string() };
    save_meta(&dir, |m| m.staging.status = status.clone())?;
    if opens && st.live.at == 0 {
        let _ = start_staging_check(&app, &state, &id);
    }
    Ok(status)
}

/// 上线时不让搜索引擎收录的主机名：迁移中的测试域名。
pub fn noindex_hosts(site_dir: &Path) -> Vec<String> {
    let st: Meta = read_json(&site_dir.join("migrate/meta.json")).unwrap_or_default();
    if st.staging.host.is_empty() { Vec::new() } else { vec![st.staging.host] }
}

// ───────────────────────── ⑦ 上线后：旧域名跳到新域名 ─────────────────────────

/// 换了域名上线以后：让旧域名（根域名和 www）的每个地址 301 到新域名的同一地址。
/// 用旧域名所在 Cloudflare 区域的重定向规则；规则只对经过代理的访问生效，没开代理的记录会打开代理（撤销时关回去）。
#[tauri::command]
pub async fn migrate_old_redirect(state: tauri::State<'_, AppState>, id: String) -> Result<OldRedirect, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let inv: Inventory = read_json(&dir.join("inventory.json")).ok_or("还没盘点")?;
    let meta = load_meta(&dir);
    if meta.cutover.at == 0 {
        return Err("还没正式上线".into());
    }
    let to = meta.cutover.host.clone();
    let old = origin_host(&inv, &site);
    if old.trim_start_matches("www.") == to.trim_start_matches("www.") {
        return Err("正式域名就是旧站的域名，不用跳转".into());
    }
    let apex = apex_of(&old);
    let (cf, token, zone) = cf_zone_for(&state, &apex).await.ok_or_else(|| format!("{apex} 不在你的 Cloudflare 账号里（或者还没生效），Pilot 没法替你设置跳转"))?;
    // 根域名和 www：有记录的才跳（没记录的本来就打不开）
    let recs = crate::cf::all_dns_records(&token, &zone.id).await?;
    let mut from = Vec::new();
    let mut flipped = Vec::new();
    for name in [apex.clone(), format!("www.{apex}")] {
        let mine: Vec<&FullRec> = recs.iter().filter(|r| r.name.eq_ignore_ascii_case(&name) && matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME")).collect();
        if mine.is_empty() || name.eq_ignore_ascii_case(&to) {
            continue;
        }
        from.push(name);
        for r in mine.into_iter().filter(|r| !r.proxied) {
            flipped.push(r.id.clone());
        }
    }
    if from.is_empty() {
        return Err(format!("{apex} 没有指向任何地方的记录，不用跳转"));
    }
    let expr = from.iter().map(|h| format!("http.host eq \"{h}\"")).collect::<Vec<_>>().join(" or ");
    let rule = serde_json::json!({
        "description": format!("{REDIRECT_TAG}: {} → {to}", from.join(" / ")),
        "expression": format!("({expr})"),
        "action": "redirect",
        "action_parameters": { "from_value": { "status_code": 301, "preserve_query_string": true, "target_url": { "expression": format!("concat(\"https://{to}\", http.request.uri.path)") } } },
        "enabled": true,
    });
    let mut rules: Vec<serde_json::Value> = crate::cf::redirect_rules(&token, &zone.id).await?.into_iter().filter(|r| !r["description"].as_str().unwrap_or("").starts_with(REDIRECT_TAG)).collect();
    rules.push(rule);
    crate::cf::put_redirect_rules(&token, &zone.id, rules).await?;
    for rid in &flipped {
        crate::cf::set_dns_proxied(&token, &zone.id, rid, true).await?;
    }
    let mut r = OldRedirect { at: now(), from: from.clone(), to: to.clone(), zone_id: zone.id.clone(), cf_conn: cf.id.clone(), proxied: flipped, verified: false, note: String::new() };
    save_meta(&dir, |m| m.old_redirect = r.clone())?;
    // 实测：拿一个旧地址看是不是 301 到新域名的同一地址（规则生效要几秒）
    let probe = read_json::<mx::UrlMap>(&PathBuf::from(&site.local_dir).join(".pilot/migrate/urls.json"))
        .and_then(|m| m.entries.into_iter().find(|e| e.action == "keep" && e.old != "/").map(|e| e.old))
        .unwrap_or_else(|| "/".into());
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10)).build().map_err(|e| e.to_string())?;
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        if let Ok(resp) = client.get(format!("https://{}{probe}", from[0])).send().await {
            let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            if resp.status().as_u16() == 301 && loc == format!("https://{to}{probe}") {
                r.verified = true;
                break;
            }
            r.note = format!("https://{}{probe} 现在返回 {} {loc}", from[0], resp.status().as_u16());
        }
    }
    if r.verified {
        r.note.clear();
    }
    save_meta(&dir, |m| m.old_redirect = r.clone())?;
    Ok(r)
}

#[tauri::command]
pub async fn migrate_old_redirect_undo(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let dir = mdir(&state, &id);
    let r = load_meta(&dir).old_redirect;
    if r.at == 0 {
        return Err("没有设置过跳转".into());
    }
    undo_old_redirect(&state, &dir, &r).await?;
    Ok(format!("{} 不再跳转，又会打开旧站了", r.from.join("、")))
}

async fn undo_old_redirect(state: &AppState, dir: &Path, r: &OldRedirect) -> Result<(), String> {
    let cf = state.conns.get(&r.cf_conn)?;
    let token = crate::keychain::get_key(&cf.id)?;
    let rules: Vec<serde_json::Value> = crate::cf::redirect_rules(&token, &r.zone_id).await?.into_iter().filter(|x| !x["description"].as_str().unwrap_or("").starts_with(REDIRECT_TAG)).collect();
    crate::cf::put_redirect_rules(&token, &r.zone_id, rules).await?;
    for rid in &r.proxied {
        let _ = crate::cf::set_dns_proxied(&token, &r.zone_id, rid, false).await;
    }
    save_meta(dir, |m| m.old_redirect = OldRedirect::default())?;
    Ok(())
}

// ───────────────────────── ⑥ 切换（自己的服务器） ─────────────────────────

/// 自己的服务器：域名现在解析到哪（DoH），和服务器 IP 对不对得上，域名在不在 Cloudflare 里（在就能自动改）。
#[derive(Serialize)]
pub struct VpsDns {
    pub host: String,
    pub ip: String,
    pub host_ips: Vec<String>,
    pub alias: String,
    pub alias_ips: Vec<String>,
    pub points: bool,
    pub alias_points: bool,
    /// 域名在已连接的 Cloudflare 账号里（切换时 Pilot 自动改 A 记录）
    pub cf_auto: bool,
}

async fn resolve_a(name: &str) -> Vec<String> {
    let mut v: Vec<String> = doh(name, "A").await.into_iter().map(|r| r.content).collect();
    if v.is_empty() {
        // CNAME 到别处的：看最终的 A
        if let Some(c) = doh(name, "CNAME").await.first() {
            v = doh(&c.content, "A").await.into_iter().map(|r| r.content).collect();
        }
    }
    v
}

async fn cf_zone_for(state: &AppState, host: &str) -> Option<(crate::conns::Connection, String, crate::cf::CfZone)> {
    for cf in state.conns.list().into_iter().filter(|c| c.kind == "cloudflare") {
        let Ok(token) = crate::keychain::get_key(&cf.id) else { continue };
        if let Ok(Some(z)) = crate::cf::zone_for(&token, host).await {
            if crate::cf::zone_info(&token, &z.id).await.map(|x| x.0 == "active").unwrap_or(false) {
                return Some((cf, token, z));
            }
        }
    }
    None
}

#[tauri::command]
pub async fn migrate_vps_dns(state: tauri::State<'_, AppState>, id: String) -> Result<VpsDns, String> {
    let v = crate::site_vps_host::load(&state.sites.data_dir(&id)).ok_or("服务器还没配置")?;
    let host_ips = resolve_a(&v.host).await;
    let alias_ips = if v.alias.is_empty() { Vec::new() } else { resolve_a(&v.alias).await };
    Ok(VpsDns {
        points: !v.ip.is_empty() && host_ips.iter().all(|i| i == &v.ip) && !host_ips.is_empty(),
        alias_points: !v.ip.is_empty() && !alias_ips.is_empty() && alias_ips.iter().all(|i| i == &v.ip),
        cf_auto: cf_zone_for(&state, &v.host).await.is_some(),
        host: v.host,
        ip: v.ip,
        host_ips,
        alias: v.alias,
        alias_ips,
    })
}

/// 切到自己的服务器：域名在 Cloudflare 里就由 Pilot 改 A 记录（不开代理，大陆访问不绕路），不在就要用户先自己改好；
/// 域名指过来以后打开 Caddy 的自动 HTTPS，等它拿到证书。
async fn cutover_vps(state: &AppState, site: &Site, dir: &Path, host: &str) -> Result<Cutover, String> {
    let site_dir = dir.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf());
    let v = crate::site_vps_host::load(&site_dir).filter(|v| v.setup_at > 0 && !v.ip.is_empty()).ok_or("服务器还没配置好（没有拿到服务器 IP）")?;
    let conn = state.conns.get(&site.deploy.ssh_conn_id).map_err(|_| "部署用的 SSH 连接不存在".to_string())?;
    let mut cut = Cutover { at: now(), host: host.to_string(), ..Default::default() };
    let names: Vec<String> = [host.to_string(), v.alias.clone()].into_iter().filter(|n| !n.is_empty()).collect();
    if let Some((cf, token, zone)) = cf_zone_for(state, host).await {
        cut.zone_id = zone.id.clone();
        cut.cf_conn = cf.id.clone();
        cut.before = crate::cf::all_dns_records(&token, &zone.id)
            .await?
            .into_iter()
            .filter(|r| names.iter().any(|n| r.name.eq_ignore_ascii_case(n)) && matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME"))
            .collect();
        save_meta(dir, |m| m.cutover = Cutover { at: 0, ..cut.clone() })?;
        for r in &cut.before {
            crate::cf::delete_dns_record(&token, &zone.id, &r.id).await?;
        }
        for n in &names {
            crate::cf::create_dns_record(&token, &zone.id, &FullRec { kind: "A".into(), name: n.clone(), content: v.ip.clone(), proxied: false, ttl: 300, ..Default::default() }).await?;
        }
        // 等公共 DNS 看到新记录（Cloudflare 一般几秒）
        for _ in 0..12 {
            if resolve_a(host).await.iter().any(|i| i == &v.ip) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        cut.notes.push(format!("已在 Cloudflare 把 {} 的 A 记录改成 {}（不开代理），原来的 {} 条记录存下来了，可以一键回退", names.join("、"), v.ip, cut.before.len()));
    } else {
        // 域名不在 Cloudflare：先看用户改好了没有
        let ips = resolve_a(host).await;
        if !ips.iter().any(|i| i == &v.ip) {
            return Err(format!("{host} 现在解析到 {}，还没指向服务器 {}：到域名的 DNS 服务商把 A 记录改成 {}（www 也改），改好后再点「正式上线」", if ips.is_empty() { "（没有记录）".to_string() } else { ips.join("、") }, v.ip, v.ip));
        }
        cut.manual_dns = true;
        // 记下切换前别人查到的旧记录（回退时提示用户改回去）
        cut.before = load_meta(dir).dns.snapshot.into_iter().filter(|r| names.iter().any(|n| r.name.eq_ignore_ascii_case(n)) && matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME")).collect();
    }
    let alias_ok = !v.alias.is_empty() && resolve_a(&v.alias).await.iter().any(|i| i == &v.ip);
    let got = crate::site_vps_host::set_https(&conn, &site_dir, true, alias_ok, None).await?;
    cut.notes.push(if got { format!("HTTPS 证书已经由 Caddy 申请好（Let's Encrypt，会自动续期）") } else { "Caddy 还在申请 HTTPS 证书（DNS 刚改，可能要几分钟），拿到后自动生效".into() });
    if !v.alias.is_empty() && !alias_ok {
        cut.notes.push(format!("{} 还没指向服务器：改好 A 记录后，它会自动跳到 {host}", v.alias));
    }
    cut.notes.push("旧站的服务器先别关：新站稳定两周左右再停".into());
    if v.api_port > 0 {
        cut.notes.push("表单和评论在服务器上照常可用，提交和评论在概览的「表单和评论」里看".into());
    }
    state.sites.update(&site.id, now(), |s| {
        s.domain = host.to_string();
        s.origin = "migrated".into();
        Ok(())
    })?;
    save_meta(dir, |m| m.cutover = cut.clone())?;
    Ok(cut)
}

async fn rollback_vps(state: &AppState, site: &Site, dir: &Path, cut: &Cutover) -> Result<String, String> {
    let site_dir = dir.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf());
    let mut msg;
    if !cut.manual_dns && !cut.zone_id.is_empty() {
        let cf = state.conns.get(&cut.cf_conn)?;
        let token = crate::keychain::get_key(&cf.id)?;
        let names: Vec<String> = cut.before.iter().map(|r| r.name.to_ascii_lowercase()).chain([cut.host.to_ascii_lowercase()]).collect();
        for r in crate::cf::all_dns_records(&token, &cut.zone_id).await?.iter().filter(|r| names.contains(&r.name.to_ascii_lowercase()) && matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME")) {
            crate::cf::delete_dns_record(&token, &cut.zone_id, &r.id).await?;
        }
        for r in &cut.before {
            crate::cf::create_dns_record(&token, &cut.zone_id, r).await?;
        }
        msg = format!("已在 Cloudflare 恢复 {} 条原来的记录，打开的又是旧站了（DNS 缓存可能要几分钟）", cut.before.len());
    } else {
        let old: Vec<String> = cut.before.iter().map(|r| format!("{} {} → {}", r.kind, r.name, r.content)).collect();
        msg = if old.is_empty() { "DNS 是你自己改的：到域名的 DNS 服务商把 A 记录改回旧主机的地址".to_string() } else { format!("DNS 是你自己改的：到域名的 DNS 服务商改回原来的记录：{}", old.join("；")) };
    }
    // 服务器上关掉 HTTPS（域名不再指过来，免得 Caddy 一直续期失败）
    if let Ok(conn) = state.conns.get(&site.deploy.ssh_conn_id) {
        if let Err(e) = crate::site_vps_host::set_https(&conn, &site_dir, false, false, None).await {
            msg.push_str(&format!("；服务器上关 HTTPS 没成功（{e}），不影响回退"));
        }
    }
    let host = cut.host.clone();
    state.sites.update(&site.id, now(), |s| {
        s.domain.clear();
        s.origin = "migrating".into();
        if s.migrate_from.is_empty() {
            s.migrate_from = host.clone();
        }
        Ok(())
    })?;
    save_meta(dir, |m| {
        m.cutover.at = 0;
        m.cutover.rolled_back_at = now();
        m.track = Track::default();
    })?;
    Ok(msg)
}

// ───────────────────────── ⑦ 跟踪 ─────────────────────────

const TRACK_STEPS: &[(&str, &str)] = &[("urls", "检查旧地址"), ("search", "对比搜索流量"), ("submit", "提交 sitemap 和 IndexNow")];

/// 切换后的跟踪：旧地址在正式域名上是否都正常、Search Console 点击和切换前比、提交 sitemap / IndexNow。
#[tauri::command]
pub async fn migrate_track(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let site = state.sites.get(&id)?;
    let dir = mdir(&state, &id);
    let meta = load_meta(&dir);
    if meta.cutover.at == 0 || site.domain.is_empty() {
        return Err("还没正式上线".into());
    }
    let job = state.site_jobs.start(Some(app), &id, "migrate:track", "迁移跟踪", TRACK_STEPS)?;
    let job_id = job.id.clone();
    let conns = state.conns.clone();
    tauri::async_runtime::spawn(async move {
        let r = run_track(site, dir, meta, conns, &job).await;
        job.finish(&r);
    });
    Ok(job_id)
}

async fn run_track(site: Site, dir: PathBuf, meta: Meta, conns: crate::conns::ConnStore, job: &Job) -> Result<(), String> {
    let code = PathBuf::from(&site.local_dir);
    let map: mx::UrlMap = read_json(&code.join(".pilot/migrate/urls.json")).ok_or("仓库里没有 .pilot/migrate/urls.json")?;
    let mut t = meta.track.clone();
    t.notes.clear();
    job.step("urls", "running", "");
    let health = crate::site_dynamic::load(&dir.parent().map(Path::to_path_buf).unwrap_or_default()).is_some_and(|d| d.setup_at > 0 && (d.needs_db() || d.needs_r2()));
    let live = live_check(&format!("https://{}", site.domain), &map, health, job).await;
    t.checked = live.checked;
    t.bad = live.bad;
    job.step("urls", if t.bad.is_empty() { "ok" } else { "failed" }, &format!("{} 个地址，{} 个不对", t.checked, t.bad.len()));

    job.step("search", "running", "");
    if site.data.gsc_site.is_empty() {
        job.step("search", "skipped", "没有接 Search Console");
    } else if let Ok(token) = crate::site_data::google_token(&conns, site.data.gsc_conn(), &crate::google::read_scopes()).await {
        let cut_day = chrono::DateTime::from_timestamp(meta.cutover.at as i64, 0).map(|d| d.date_naive()).unwrap_or_default();
        let start = cut_day - chrono::Duration::days(28);
        let end = chrono::Local::now().date_naive() - chrono::Duration::days(2);
        match crate::google::gsc_query(&token, &site.data.gsc_site, &start.to_string(), &end.to_string(), &["date"], 200).await {
            Ok(rows) => {
                let (mut b, mut bn, mut a, mut an) = (0.0, 0u32, 0.0, 0u32);
                for r in rows {
                    let Some(d) = r.keys.first().and_then(|k| chrono::NaiveDate::parse_from_str(k, "%Y-%m-%d").ok()) else { continue };
                    if d < cut_day {
                        b += r.clicks;
                        bn += 1;
                    } else {
                        a += r.clicks;
                        an += 1;
                    }
                }
                t.base_clicks = if bn > 0 { b / bn as f64 } else { 0.0 };
                t.after_clicks = if an > 0 { a / an as f64 } else { 0.0 };
                t.after_days = an;
                if an >= 3 && t.base_clicks > 5.0 && t.after_clicks < t.base_clicks * 0.7 {
                    t.notes.push(format!("上线后日均点击 {:.0}，比上线前（{:.0}）低了 {:.0}%：看看上面不对的地址，以及 Search Console 的「网页索引编制」", t.after_clicks, t.base_clicks, (1.0 - t.after_clicks / t.base_clicks) * 100.0));
                }
                job.step("search", "ok", &format!("上线前日均 {:.1}，上线后 {:.1}（{an} 天）", t.base_clicks, t.after_clicks));
            }
            Err(e) => job.step("search", "failed", &e),
        }
    } else {
        job.step("search", "failed", "读不到 Search Console");
    }

    job.step("submit", "running", "");
    let keep: Vec<String> = map.entries.iter().filter(|e| e.action == "keep" && !matches!(e.kind.as_str(), "file")).map(|e| format!("https://{}{}", site.domain, e.old)).collect();
    if !t.sitemap_submitted && !site.data.gsc_site.is_empty() {
        if let Ok(token) = crate::site_data::google_token(&conns, site.data.gsc_conn(), &[crate::google::SCOPE_WEBMASTERS]).await {
            for f in ["sitemap-index.xml", "sitemap.xml"] {
                let url = format!("https://{}/{f}", site.domain);
                if fetch(&client(), &url, true).await.map(|r| r.status == 200).unwrap_or(false) {
                    if crate::google::gsc_submit_sitemap(&token, &site.data.gsc_site, &url).await.is_ok() {
                        t.sitemap_submitted = true;
                    }
                    break;
                }
            }
        }
    }
    if !t.indexnow_submitted && !site.data.indexnow_key.is_empty() {
        let key_url = format!("https://{}/{}.txt", site.domain, site.data.indexnow_key);
        if fetch(&client(), &key_url, false).await.map(|r| r.status == 200 && r.body.trim() == site.data.indexnow_key).unwrap_or(false) {
            if crate::bing::indexnow_submit(&site.domain, &site.data.indexnow_key, &keep).await.is_ok() {
                t.indexnow_submitted = true;
            }
        }
    }
    job.step("submit", "ok", &format!("sitemap {}，IndexNow {}", if t.sitemap_submitted { "已提交" } else { "未提交" }, if t.indexnow_submitted { "已提交" } else { "未提交" }));
    t.at = now();
    save_meta(&dir, |m| m.track = t)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuild_message_says_the_domain_and_ga_are_settled() {
        let inv = Inventory { domain: "old.example".into(), origin: "https://www.old.example/".into(), analytics: vec!["UA-1".into(), "G-ABC123".into()], ..Default::default() };
        let m = rebuild_message("旧站", &inv, &Plan::default(), &Meta::default(), false);
        assert!(m.contains("正式域名就是 www.old.example"));
        assert!(m.contains("不要把正式域名写进待办"));
        assert!(m.contains("GA4（G-ABC123）Pilot 已经存成配置变量 `PUBLIC_GA_ID`"));
        let m = rebuild_message("旧站", &Inventory { domain: "old.example".into(), ..Default::default() }, &Plan::default(), &Meta::default(), false);
        assert!(m.contains("正式域名就是 old.example") && m.contains("旧站没有 GA4"));
    }

    #[test]
    fn hosts_are_cleaned_and_formal_defaults_to_the_old_one() {
        assert_eq!(clean_host(" https://New.Example.com/path ").unwrap(), "new.example.com");
        assert!(clean_host("localhost").is_err() && clean_host("a..b").is_err() && clean_host("-a.com").is_err());
        let inv = Inventory { origin: "https://www.old.example/".into(), ..Default::default() };
        assert_eq!(origin_host_or(&inv, "old.example"), "www.old.example");
        assert_eq!(origin_host_or(&Inventory::default(), "old.example"), "old.example", "盘点里没有地址就用旧域名");
    }

    #[test]
    fn apex() {
        assert_eq!(apex_of("www.example.com"), "example.com");
        assert_eq!(apex_of("example.com"), "example.com");
        assert_eq!(apex_of("www.example.com.cn"), "example.com.cn");
        assert_eq!(apex_of("a.b.example.co.uk"), "example.co.uk");
    }

    #[test]
    fn names() {
        assert_eq!(name_from_title("Example — Best widgets", "e.com"), "Example");
        assert_eq!(name_from_title("", "e.com"), "e.com");
    }

    #[test]
    fn decodes_gbk() {
        let (bytes, _, _) = encoding_rs::GBK.encode("<meta charset=\"gbk\">你好");
        assert!(decode(&bytes, "text/html").contains("你好"));
        assert!(decode("你好".as_bytes(), "text/html; charset=utf-8").contains("你好"));
    }

    /// 手动跑：在真实站点上走一遍抓取 → 分组 → 正文提取 → 对应表（不进 CI）。
    /// MIGRATE_DOMAIN=example.com cargo test --lib live_crawl -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_crawl() {
        let domain = std::env::var("MIGRATE_DOMAIN").unwrap_or_else(|_| "htxneo.com".into());
        let limit: usize = std::env::var("MIGRATE_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
        let c = client();
        let (home, _) = fetch_follow(&c, &format!("https://{domain}/")).await.expect("home");
        let origin = Url::parse(&format!("{}://{}/", home.scheme(), home.host_str().unwrap())).unwrap();
        println!("origin {origin}");
        let tmp = tempfile::tempdir().unwrap();
        let mut cr = Crawl {
            c: c.clone(), origin: origin.clone(), domain: domain.clone(), robots: sm::Robots::default(), snap_dir: tmp.path().to_path_buf(),
            pages: BTreeMap::new(), found: BTreeMap::new(), queue: VecDeque::new(), links_in: HashMap::new(), assets: BTreeMap::new(),
            analytics: Vec::new(), generator: String::new(), truncated: false,
        };
        if let Ok(f) = fetch(&c, &cr.url_of("/robots.txt"), false).await {
            if f.status == 200 { cr.robots = sm::parse_robots(&f.body); }
        }
        cr.enqueue("/".into(), "crawl");
        let mut maps = cr.robots.sitemaps.clone();
        maps.push(cr.url_of("/sitemap-index.xml"));
        maps.push(cr.url_of("/sitemap.xml"));
        let mut seen = HashSet::new();
        while let Some(m) = maps.pop() {
            if !seen.insert(m.clone()) { continue; }
            let Ok((_, f)) = fetch_follow(&c, &m).await else { continue };
            if f.status != 200 { continue; }
            let (urls, subs) = sm::parse_sitemap(&f.body);
            println!("sitemap {m}: {} urls, {} subs", urls.len(), subs.len());
            maps.extend(subs.into_iter().filter(|s| !sm::is_media_sitemap(s)));
            if sm::is_media_sitemap(&m) { continue; }
            for u in urls { if let Ok(u) = Url::parse(&u) { cr.enqueue(sm::path_of(&u), "sitemap"); } }
        }
        while let Some(path) = cr.queue.pop_front() {
            if cr.pages.len() >= limit { break; }
            let (mut page, html) = crawl_one(c.clone(), origin.to_string(), path, domain.clone()).await;
            if let Some(h) = html { cr.take_html(&mut page, &h, false); }
            page.group = sm::group_of(&page.path);
            cr.pages.insert(page.path.clone(), page);
        }
        let mut pages: Vec<Page> = cr.pages.values().cloned().collect();
        let suffix = sm::tidy_titles(&mut pages);
        println!("pages {} queue left {} title suffix {suffix:?}", pages.len(), cr.queue.len());
        for p in &pages { println!("  {:8} {:3} {:8} {:30} {}", p.kind, p.status, p.ptype, p.group, p.path); }
        println!("trailing {}", sm::slash_style(pages.iter().filter(|p| p.kind == "page").map(|p| p.path.as_str())));
        let feats = sm::summarize_features(&pages);
        println!("features {:?}", feats.iter().map(|f| (&f.key, f.pages)).collect::<Vec<_>>());
        println!("analytics {:?} generator {:?} assets {}", cr.analytics, cr.generator, cr.assets.len());
        let sigs: Vec<(String, Vec<u64>)> = pages.iter().filter(|p| !p.snapshot.is_empty()).filter_map(|p| gz_read(&tmp.path().join(&p.snapshot)).map(|h| (p.group.clone(), mx::block_sigs(&h)))).collect();
        let boiler = mx::boilerplate(&sigs);
        println!("boilerplate blocks {}", boiler.len());
        let mut shown = 0;
        for p in pages.iter().filter(|p| p.kind == "page" && matches!(p.ptype.as_str(), "article" | "page")) {
            let h = gz_read(&tmp.path().join(&p.snapshot)).unwrap();
            let u = Url::parse(&format!("{}{}", origin.as_str().trim_end_matches('/'), p.path)).unwrap();
            let ct = mx::extract_content(&h, &u, &domain, &boiler);
            let cm = mx::extract_comments(&h);
            println!("  {:.2} {:5}w {} -> {:?}  comments {}", ct.coverage, sm::word_count(&ct.text), p.path, mx::content_path(&p.path), cm.len());
            if std::env::var("MIGRATE_COMMENTS").is_ok() { for c in cm.iter().take(4) { println!("      #{} ^{} {} @{} :: {}", c.id, c.parent, c.author, c.date, c.content.chars().take(80).collect::<String>()); } }
            let want = std::env::var("MIGRATE_SHOW").unwrap_or_default();
            if (want.is_empty() && shown < 1 && ct.markdown.len() > 200) || (!want.is_empty() && p.path == want) {
                shown += 1;
                println!("----- {} -----\n{}{}\n-----", p.path, mx::frontmatter(p, "", &suffix), if want.is_empty() { ct.markdown.chars().take(1500).collect::<String>() } else { ct.markdown.clone() });
            }
        }
        let map = mx::build_url_map(&Inventory { domain: domain.clone(), pages: pages.clone(), ..Default::default() }, &Plan::default(), &HashMap::new());
        println!("urlmap {} entries ({} keep)", map.entries.len(), map.entries.iter().filter(|e| e.action == "keep").count());
    }

    /// 手动跑：真实站点上走「盘点（简化）→ 抓取 → 核对」。核对拿旧站自己的页面当新站，应该全部通过（检验核对规则不误报）。
    /// MIGRATE_DOMAIN=example.com cargo test --lib live_extract_parity -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_extract_parity() {
        let domain = std::env::var("MIGRATE_DOMAIN").unwrap_or_else(|_| "htxneo.com".into());
        let limit: usize = std::env::var("MIGRATE_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
        let c = client();
        let (home, _) = fetch_follow(&c, &format!("https://{domain}/")).await.expect("home");
        let origin = Url::parse(&format!("{}://{}/", home.scheme(), home.host_str().unwrap())).unwrap();
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let code = root.path().join("code");
        std::fs::create_dir_all(data.join("snapshots")).unwrap();
        std::fs::create_dir_all(code.join(".pilot/migrate")).unwrap();
        let mut cr = Crawl {
            c: c.clone(), origin: origin.clone(), domain: domain.clone(), robots: sm::Robots::default(), snap_dir: data.join("snapshots"),
            pages: BTreeMap::new(), found: BTreeMap::new(), queue: VecDeque::new(), links_in: HashMap::new(), assets: BTreeMap::new(),
            analytics: Vec::new(), generator: String::new(), truncated: false,
        };
        cr.enqueue("/".into(), "crawl");
        if let Ok((_, f)) = fetch_follow(&c, &cr.url_of("/sitemap.xml")).await {
            for u in sm::parse_sitemap(&f.body).0 { if let Ok(u) = Url::parse(&u) { cr.enqueue(sm::path_of(&u), "sitemap"); } }
        }
        while let Some(path) = cr.queue.pop_front() {
            if cr.pages.len() >= limit { break; }
            let (mut page, html) = crawl_one(c.clone(), origin.to_string(), path, domain.clone()).await;
            if let Some(h) = html { cr.take_html(&mut page, &h, false); }
            page.group = sm::group_of(&page.path);
            cr.pages.insert(page.path.clone(), page);
        }
        let mut pages: Vec<Page> = cr.pages.values().cloned().collect();
        let title_suffix = sm::tidy_titles(&mut pages);
        let inv = Inventory {
            domain: domain.clone(), origin: origin.as_str().trim_end_matches('/').into(),
            trailing_slash: sm::slash_style(pages.iter().filter(|p| p.kind == "page").map(|p| p.path.as_str())).into(),
            assets: cr.assets.clone(), title_suffix, pages, ..Default::default()
        };
        let plan = Plan { confirmed_at: 1, ..Default::default() };
        let site = Site {
            id: "t".into(), name: "t".into(), domain: String::new(), source: "local".into(), gh_conn_id: String::new(), repo: String::new(),
            local_dir: code.to_string_lossy().into(), default_branch: "main".into(), framework: "astro".into(),
            build: Default::default(), deploy: Default::default(), data: Default::default(), ops: Default::default(),
            pinned: false, tags: vec![], origin: "migrating".into(), migrate_from: domain.clone(), created_at: 0, updated_at: 0,
        };
        let jobs = crate::site_jobs::Jobs::default();
        let job = jobs.start(None, "t", "migrate:extract", "抓取", EXTRACT_STEPS).unwrap();
        run_extract(site, inv.clone(), plan, data.clone(), &job).await.expect("extract");
        for st in jobs.snapshot("t").unwrap().steps { println!("step {} {} {}", st.key, st.status, st.detail); }
        let meta = load_meta(&data);
        println!("extract: {} pages, {} lists, {} assets, {} failed, low {:?}", meta.extract.pages, meta.extract.lists, meta.extract.assets, meta.extract.assets_failed.len(), meta.extract.low_coverage);
        let map: mx::UrlMap = read_json(&code.join(".pilot/migrate/urls.json")).unwrap();
        println!("urls.json {} entries; content files: {}", map.entries.len(), map.entries.iter().filter(|e| !e.file.is_empty()).count());
        // 旧站自己当「新站」：快照按 Cloudflare Pages 的文件规则摆好，图片也放进去
        let out = root.path().join("out");
        for p in inv.html_pages() {
            let Some(h) = gz_read(&data.join("snapshots").join(&p.snapshot)) else { continue };
            let rel = mx::pct_decode(p.path.trim_start_matches('/'));
            let f = if rel.is_empty() { out.join("index.html") } else if rel.ends_with('/') { out.join(&rel).join("index.html") } else { out.join(format!("{rel}.html")) };
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, h).unwrap();
        }
        let _ = copy_dir(&code.join("public"), &out);
        if let Ok((_, f)) = fetch_follow(&c, &cr.url_of("/sitemap.xml")).await { std::fs::write(out.join("sitemap.xml"), f.body).unwrap(); }
        let assets: Vec<String> = read_json(&code.join(".pilot/migrate/assets.json")).unwrap_or_default();
        for chk in mx::parity(&out, &map, &inv, &data.join("ref"), &assets, &[]) {
            println!("parity {:5} {:20} {}", chk.level, chk.label, chk.detail);
            for i in chk.items.iter().take(5) { println!("        {i}"); }
        }
        let one = map.entries.iter().find(|e| !e.file.is_empty()).unwrap();
        println!("----- {} -----\n{}", one.file, std::fs::read_to_string(code.join(&one.file)).unwrap().chars().take(600).collect::<String>());
    }

    fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
        for e in std::fs::read_dir(from)? {
            let e = e?;
            let t = to.join(e.file_name());
            if e.file_type()?.is_dir() { std::fs::create_dir_all(&t)?; copy_dir(&e.path(), &t)?; } else { std::fs::copy(e.path(), t)?; }
        }
        Ok(())
    }

    #[test]
    fn stages() {
        let m = Meta::default();
        assert_eq!(stage(None, None, &m), "survey");
        let inv = Inventory::default();
        let m = Meta { surveyed_at: 1, ..Default::default() };
        assert_eq!(stage(Some(&inv), None, &m), "plan");
        let p = Plan { confirmed_at: 1, ..Default::default() };
        assert_eq!(stage(Some(&inv), Some(&p), &m), "extract");
        let m = Meta { surveyed_at: 1, extracted_at: 1, ..Default::default() };
        assert_eq!(stage(Some(&inv), Some(&p), &m), "rebuild");
        let m = Meta { surveyed_at: 1, extracted_at: 1, parity_at: 1, ..Default::default() };
        assert_eq!(stage(Some(&inv), Some(&p), &m), "verify");
        let m = Meta { surveyed_at: 1, extracted_at: 1, parity_at: 1, live: LiveCheck { at: 1, ..Default::default() }, ..Default::default() };
        assert_eq!(stage(Some(&inv), Some(&p), &m), "cutover");
    }
}
