//! 站点运营：机会规则（Pilot 按固定规则从缓存数据里算，可解释、可复现）、
//! 待处理队列、改动分类（第 4 档白名单由 Pilot 按文件差异判定，不信任模型自报的类型）。
//! 运行器（扫描 / 体检 / 周报 / 复盘、调度）见 site_runner。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::site_data::{cache_dir, read_json, Row};
use crate::sites::Site;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Opportunity {
    /// 稳定键：类型 + 页面（+ 查询词），用于去重与「驳回后不再提」。
    pub key: String,
    /// low_ctr | striking | declining | content_gap | crawl_issue | sitemap_error
    pub kind: String,
    pub title: String,
    pub page: String,
    #[serde(default)]
    pub queries: Vec<Row>,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
    #[serde(default)]
    pub prev_clicks: f64,
    /// 粗略的「每月可多拿到的点击」，用于排序。
    pub score: f64,
    /// 预计的改动类型：meta | content | internal_links | new_content | technical
    pub change: String,
    /// 给人和模型看的依据（不含凭据；数字只用于判断，不写进仓库）。
    pub evidence: String,
    /// 这条是从哪来的：data（数据规则 / 数据）| gap（内容缺口）| topic（选题清单）| positioning（按站点定位推导）
    /// | health（体检）| manual（手动加入）。空＝老数据，按 kind 推断。
    #[serde(default)]
    pub source: String,
    /// 目标查询词（新内容、计划项里 AI 给的）。
    #[serde(default)]
    pub target: String,
}

/// 各排名位置的大致点击率（用于判断「点击率是否明显偏低」，只求量级正确）。
pub fn expected_ctr(position: f64) -> f64 {
    const CURVE: [f64; 10] = [0.28, 0.15, 0.10, 0.07, 0.05, 0.04, 0.03, 0.025, 0.02, 0.018];
    if position < 1.0 {
        return CURVE[0];
    }
    let i = position.floor() as usize;
    if i >= 10 {
        return (0.018 * 10.0 / position).max(0.002);
    }
    let lo = CURVE[i - 1];
    let hi = CURVE.get(i).copied().unwrap_or(0.016);
    lo + (hi - lo) * (position - position.floor())
}

fn pct(v: f64) -> String {
    format!("{:.1}%", v * 100.0)
}

fn path_of(url: &str) -> String {
    url.split_once("://")
        .map(|(_, rest)| rest.find('/').map(|i| rest[i..].to_string()).unwrap_or_else(|| "/".into()))
        .unwrap_or_else(|| url.to_string())
}

/// 从缓存算出机会列表（按 score 降序，每类最多 10 条）。没有数据就是空列表。
pub fn opportunities(site: &Site, site_dir: &Path) -> Vec<Opportunity> {
    let dir = cache_dir(site_dir);
    let min_impr = site.ops.min_impressions.max(10) as f64;
    let qp: Vec<Row> = read_json(&dir, "gsc_query_page.json");
    let pages: Vec<Row> = read_json(&dir, "gsc_pages.json");
    let pages_prev: Vec<Row> = read_json(&dir, "gsc_pages_prev.json");
    let queries: Vec<Row> = read_json(&dir, "gsc_queries.json");
    let mut out: Vec<Opportunity> = Vec::new();

    // 页面 → 它的查询词
    let mut by_page: HashMap<&str, Vec<&Row>> = HashMap::new();
    for r in &qp {
        by_page.entry(r.key2.as_str()).or_default().push(r);
    }
    let top_queries = |page: &str, pred: &dyn Fn(&Row) -> bool| -> Vec<Row> {
        let mut v: Vec<Row> = by_page.get(page).map(|rs| rs.iter().filter(|r| pred(r)).map(|r| (*r).clone()).collect()).unwrap_or_default();
        v.sort_by(|a, b| b.impressions.total_cmp(&a.impressions));
        v.truncate(8);
        v
    };

    // 1) 排名靠前但点击率明显偏低 → 改标题 / 描述
    let mut low = Vec::new();
    for p in &pages {
        if p.impressions < min_impr || p.position > 10.0 || p.position <= 0.0 {
            continue;
        }
        let exp = expected_ctr(p.position);
        if p.ctr >= exp * 0.5 {
            continue;
        }
        let qs = top_queries(&p.key, &|r: &Row| r.position <= 12.0);
        low.push(Opportunity {
            key: format!("low_ctr:{}", p.key),
            kind: "low_ctr".into(),
            title: format!("{} 排名靠前但点击率偏低", path_of(&p.key)),
            page: p.key.clone(),
            source: "data".into(),
            target: String::new(),
            evidence: format!(
                "近 28 天平均排名 {:.1}、展现 {:.0}、点击率 {}，同排名的常见点击率约 {}。标题和描述可能没有回应搜索意图。",
                p.position, p.impressions, pct(p.ctr), pct(exp)
            ),
            queries: qs,
            clicks: p.clicks,
            impressions: p.impressions,
            ctr: p.ctr,
            position: p.position,
            prev_clicks: 0.0,
            score: p.impressions * (exp - p.ctr),
            change: "meta".into(),
        });
    }
    // 2) 排名 8–20（第一页边缘 / 第二页）→ 补内容、加内链
    let mut striking = Vec::new();
    for p in &pages {
        let qs = top_queries(&p.key, &|r: &Row| (8.0..=20.0).contains(&r.position) && r.impressions >= min_impr / 2.0);
        if qs.is_empty() {
            continue;
        }
        let impr: f64 = qs.iter().map(|q| q.impressions).sum();
        let pos: f64 = qs.iter().map(|q| q.position * q.impressions).sum::<f64>() / impr.max(1.0);
        let clicks: f64 = qs.iter().map(|q| q.clicks).sum();
        striking.push(Opportunity {
            key: format!("striking:{}", p.key),
            kind: "striking".into(),
            title: format!("{} 有 {} 个查询词排在 8–20 位", path_of(&p.key), qs.len()),
            page: p.key.clone(),
            source: "data".into(),
            target: String::new(),
            evidence: format!(
                "这些查询词近 28 天合计展现 {impr:.0}、平均排名 {pos:.1}。补充对应内容、加站内链接，有机会进到前几位。"
            ),
            queries: qs,
            clicks,
            impressions: impr,
            ctr: if impr > 0.0 { clicks / impr } else { 0.0 },
            position: pos,
            prev_clicks: 0.0,
            score: impr * (expected_ctr(5.0) - expected_ctr(pos)).max(0.0),
            change: "content".into(),
        });
    }
    // 3) 点击明显下滑（与前 28 天比）
    let prev: HashMap<&str, &Row> = pages_prev.iter().map(|r| (r.key.as_str(), r)).collect();
    let mut declining = Vec::new();
    for p in &pages {
        let Some(old) = prev.get(p.key.as_str()) else { continue };
        if old.clicks < 10.0 || p.clicks > old.clicks * 0.7 {
            continue;
        }
        declining.push(Opportunity {
            key: format!("declining:{}", p.key),
            kind: "declining".into(),
            title: format!("{} 点击下滑 {:.0}%", path_of(&p.key), (1.0 - p.clicks / old.clicks) * 100.0),
            page: p.key.clone(),
            source: "data".into(),
            target: String::new(),
            evidence: format!(
                "近 28 天点击 {:.0}（前 28 天 {:.0}），平均排名 {:.1} → {:.1}。先查是内容过时、排名被挤还是季节性。",
                p.clicks, old.clicks, old.position, p.position
            ),
            queries: top_queries(&p.key, &|_| true),
            clicks: p.clicks,
            impressions: p.impressions,
            ctr: p.ctr,
            position: p.position,
            prev_clicks: old.clicks,
            score: old.clicks - p.clicks,
            change: "content".into(),
        });
    }
    // 4) 有需求但没有像样的页面（所有页面都排在 20 名以后）
    let mut best: HashMap<&str, f64> = HashMap::new();
    for r in &qp {
        let b = best.entry(r.key.as_str()).or_insert(f64::MAX);
        if r.position < *b {
            *b = r.position;
        }
    }
    let mut gap = Vec::new();
    for q in &queries {
        if q.impressions < min_impr || best.get(q.key.as_str()).is_some_and(|b| *b <= 20.0) || q.position <= 20.0 {
            continue;
        }
        gap.push(Opportunity {
            key: format!("content_gap:{}", q.key),
            kind: "content_gap".into(),
            title: format!("「{}」有搜索需求，但没有页面排进前 20", q.key),
            page: String::new(),
            source: "gap".into(),
            target: q.key.clone(),
            evidence: format!("近 28 天展现 {:.0}，最好的页面排在 {:.0} 位左右。可以考虑写一篇专门回应它的内容。", q.impressions, q.position),
            queries: vec![q.clone()],
            clicks: q.clicks,
            impressions: q.impressions,
            ctr: q.ctr,
            position: q.position,
            prev_clicks: 0.0,
            score: q.impressions * expected_ctr(8.0),
            change: "new_content".into(),
        });
    }
    // 5) 技术问题：Bing 抓取错误、Search Console sitemap 报错
    let mut tech = Vec::new();
    let crawl: Vec<crate::bing::CrawlIssue> = read_json(&dir, "bing_crawl.json");
    for c in crawl.iter().filter(|c| c.http_code >= 400).take(20) {
        tech.push(Opportunity {
            key: format!("crawl_issue:{}", c.url),
            kind: "crawl_issue".into(),
            title: format!("Bing 抓取 {} 返回 {}", path_of(&c.url), c.http_code),
            page: c.url.clone(),
            evidence: "Bing 报告这个网址抓取失败。检查是否为死链、是否该加跳转，或从站内链接 / sitemap 里移除。".into(),
            score: 5.0,
            change: "technical".into(),
            ..Default::default()
        });
    }
    let sitemaps: serde_json::Value = read_json(&dir, "gsc_sitemaps.json");
    for s in sitemaps.as_array().cloned().unwrap_or_default() {
        let errors = s["errors"].as_str().and_then(|v| v.parse::<i64>().ok()).or(s["errors"].as_i64()).unwrap_or(0);
        if errors > 0 {
            let path = s["path"].as_str().unwrap_or_default().to_string();
            tech.push(Opportunity {
                key: format!("sitemap_error:{path}"),
                kind: "sitemap_error".into(),
                title: format!("sitemap 有 {errors} 个错误"),
                page: path,
                evidence: "Search Console 报告 sitemap 解析出错，相关页面可能不会被收录。".into(),
                score: 20.0,
                change: "technical".into(),
                ..Default::default()
            });
        }
    }

    for mut group in [low, striking, declining, gap, tech] {
        group.sort_by(|a, b| b.score.total_cmp(&a.score));
        group.truncate(10);
        out.extend(group);
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

// ---------------------------------------------------------------------------
// 待处理队列
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct QueueItem {
    pub id: String,
    pub opp: Opportunity,
    /// open（待处理）| working（AI 处理中）| proposed（有方案 / PR / 本地分支待你确认）|
    /// merged（已合并待上线）| deployed（已上线）| dismissed（已驳回）| rolled_back（已回滚）| failed
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// 处理它的会话。
    #[serde(default)]
    pub conv_id: String,
    /// 第 3、4 档：改动所在分支 / PR。
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub base_sha: String,
    #[serde(default)]
    pub head_sha: String,
    #[serde(default)]
    pub pr_number: u64,
    #[serde(default)]
    pub pr_url: String,
    /// Pilot 按差异判定的改动类型（meta_description / title / internal_links / mixed …）。
    #[serde(default)]
    pub change_kind: String,
    /// 白名单判定结论：auto（可自动上线）或不可自动上线的原因。
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub merge_sha: String,
    #[serde(default)]
    pub deployed_at: u64,
    /// 效果复盘：上线后 ~28 天对比前 28 天（positive / neutral / negative + 说明）。
    #[serde(default)]
    pub review: String,
    #[serde(default)]
    pub review_note: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub auto: bool,
    /// 交给 AI 处理的时间（算每周改动预算）。
    #[serde(default)]
    pub worked_at: u64,
    /// AI 自己对这次改动的说明（改了哪些文件、为什么这样改），截断保存。
    #[serde(default)]
    pub ai_summary: String,
    /// 这次改动涉及的文件。
    #[serde(default)]
    pub files: Vec<String>,
    /// 上线时这个页面近 28 天的表现（效果复盘的「之前」）。
    #[serde(default)]
    pub before: Option<Row>,
    #[serde(default)]
    pub after: Option<Row>,
    /// 全托管：上线后检查的时间点（0＝不需要）与结果（ok / 失败原因）。
    #[serde(default)]
    pub watch_due: u64,
    #[serde(default)]
    pub watched: String,
    /// 处理这条用的模型（备用模型顶上时会写明）。
    #[serde(default)]
    pub model_used: String,
    /// AI 报告的这次改到的已有页面、新增的页面（完整网址），上线后逐个验收。
    #[serde(default)]
    pub pages: Vec<String>,
    #[serde(default)]
    pub new_pages: Vec<String>,
    /// 这次加的几段文字，用来确认线上已经是新内容。
    #[serde(default)]
    pub snippets: Vec<String>,
    /// 最近一次线上验收的结果。
    #[serde(default)]
    pub checks: Vec<crate::site_verify::Check>,
    /// 上线了，但验收有一般问题（不回滚），等你核对；空＝没有。
    #[serde(default)]
    pub attention: String,
    /// 验收严重失败、回滚后 AI 自动重做过几次（最多 1 次）。
    #[serde(default)]
    pub retries: u32,
    /// 上次失败的原因，重做时交给 AI。
    #[serde(default)]
    pub retry_note: String,
    /// 上线后复查到第几次：0＝1 小时后，1＝24 小时后，2＝查完了。
    #[serde(default)]
    pub watch_stage: u8,
    /// 因为线上验收严重失败被回滚的次数（同类改动累计 2 次就不再自动上线）。
    #[serde(default)]
    pub verify_rollbacks: u32,
}

/// 这一条要不要人来处理：待确认、失败，或者上线了但线上验收有待核对的问题。
pub fn needs_you(i: &QueueItem) -> bool {
    matches!(i.status.as_str(), "proposed" | "failed") || (i.status == "deployed" && !i.attention.is_empty())
}

pub fn queue_path(site_dir: &Path) -> PathBuf {
    site_dir.join("queue.json")
}

static QUEUE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn load_queue(site_dir: &Path) -> Vec<QueueItem> {
    std::fs::read(queue_path(site_dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_queue(site_dir: &Path, q: &[QueueItem]) -> Result<(), String> {
    let raw = serde_json::to_vec_pretty(q).map_err(|e| e.to_string())?;
    let p = queue_path(site_dir);
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// 在锁内读改写队列。
pub fn with_queue<R>(site_dir: &Path, f: impl FnOnce(&mut Vec<QueueItem>) -> R) -> Result<R, String> {
    let _g = QUEUE_LOCK.lock().unwrap();
    let mut q = load_queue(site_dir);
    let r = f(&mut q);
    save_queue(site_dir, &q)?;
    Ok(r)
}

/// 撤掉 key 以 prefix 开头、还没处理（open）的条目，返回撤掉几条。
pub fn drop_open_with_prefix(site_dir: &Path, prefix: &str) -> Result<usize, String> {
    let q = load_queue(site_dir);
    let before = q.len();
    let kept: Vec<QueueItem> = q.into_iter().filter(|i| !(i.status == "open" && i.opp.key.starts_with(prefix))).collect();
    let n = before - kept.len();
    if n > 0 {
        save_queue(site_dir, &kept)?;
    }
    Ok(n)
}

pub fn update_item(site_dir: &Path, id: &str, now: u64, f: impl FnOnce(&mut QueueItem)) -> Result<Option<QueueItem>, String> {
    with_queue(site_dir, |q| {
        q.iter_mut().find(|i| i.id == id).map(|i| {
            f(i);
            i.updated_at = now;
            i.clone()
        })
    })
}

/// 同一个机会一个周期内只处理一次：键已在队列里（未结束）或在冷却期内结束过，就不再入队。
/// 被驳回的永远不再提（除非你在队列里把它恢复）。
pub fn should_enqueue(q: &[QueueItem], key: &str, now: u64, cooldown: u64) -> bool {
    !q.iter().any(|i| {
        i.opp.key == key
            && (matches!(i.status.as_str(), "open" | "working" | "proposed" | "merged" | "dismissed")
                || now.saturating_sub(i.updated_at) < cooldown)
    })
}

/// 把新机会并入队列，返回新增的条目。
pub fn enqueue(site_dir: &Path, opps: &[Opportunity], now: u64, cooldown: u64) -> Result<Vec<QueueItem>, String> {
    with_queue(site_dir, |q| {
        let mut added = Vec::new();
        for o in opps {
            if !should_enqueue(q, &o.key, now, cooldown) {
                continue;
            }
            let item = QueueItem {
                id: uuid::Uuid::new_v4().to_string(),
                opp: o.clone(),
                status: "open".into(),
                created_at: now,
                updated_at: now,
                ..Default::default()
            };
            q.insert(0, item.clone());
            added.push(item);
        }
        q.truncate(500);
        added
    })
}

// ---------------------------------------------------------------------------
// 改动分类（第 4 档白名单）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Classified {
    /// 命中的白名单类型（可能多个）；空＝不是任何白名单改动。
    pub kinds: Vec<String>,
    /// 不能自动上线的原因（空＝整份差异都在白名单内）。
    pub reasons: Vec<String>,
}

#[derive(Debug, Default)]
struct FileDiff {
    path: String,
    removed: Vec<String>,
    added: Vec<String>,
    new_file: bool,
    deleted: bool,
    binary: bool,
}

fn parse_diff(diff: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.split(" b/").nth(1).unwrap_or(rest).to_string();
            files.push(FileDiff { path, ..Default::default() });
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if line.starts_with("new file mode") {
            f.new_file = true;
        } else if line.starts_with("deleted file mode") {
            f.deleted = true;
        } else if line.starts_with("Binary files") {
            f.binary = true;
        } else if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") || line.starts_with("index ") {
        } else if let Some(a) = line.strip_prefix('+') {
            f.added.push(a.to_string());
        } else if let Some(r) = line.strip_prefix('-') {
            f.removed.push(r.to_string());
        }
    }
    files
}

const CONTENT_EXT: &[&str] = &["md", "mdx", "markdown", "html", "htm", "astro", "njk", "liquid", "hbs", "vue", "svelte"];

fn ext(path: &str) -> &str {
    path.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
}

/// 一行是不是「描述」字段：frontmatter 的 description: 或 <meta name="description" content="…">。
fn description_value(line: &str) -> Option<String> {
    let t = line.trim();
    if let Some(v) = t.strip_prefix("description:") {
        return Some(v.trim().trim_matches(['"', '\'']).to_string());
    }
    let low = t.to_ascii_lowercase();
    if low.contains("<meta") && (low.contains("name=\"description\"") || low.contains("name='description'") || low.contains("property=\"og:description\"")) {
        let i = low.find("content=")?;
        let rest = &t[i + 8..];
        let q = rest.chars().next()?;
        let body = &rest[1..];
        return body.find(q).map(|end| body[..end].to_string());
    }
    None
}

/// 一行是不是「标题」字段：frontmatter 的 title: 或 <title>…</title>。不包括正文里的 # 标题。
fn title_value(line: &str) -> Option<String> {
    let t = line.trim();
    if let Some(v) = t.strip_prefix("title:") {
        return Some(v.trim().trim_matches(['"', '\'']).to_string());
    }
    let low = t.to_ascii_lowercase();
    if let (Some(a), Some(b)) = (low.find("<title>"), low.find("</title>")) {
        return (a + 7 <= b).then(|| t[a + 7..b].to_string());
    }
    None
}

/// 去掉一行里的站内链接标记（只认指向站内 / 开头的链接），用于判断「只是加了链接」。
fn strip_internal_links(line: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut links = Vec::new();
    let mut rest = line;
    loop {
        // Markdown: [文字](/path)
        let md = rest.find("](/");
        let html = rest.find("<a href=\"/");
        let next = match (md, html) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let Some(pos) = next else {
            out.push_str(rest);
            break;
        };
        if Some(pos) == md {
            let Some(open) = rest[..pos].rfind('[') else {
                out.push_str(&rest[..pos + 2]);
                rest = &rest[pos + 2..];
                continue;
            };
            let Some(close) = rest[pos + 2..].find(')') else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..open]);
            out.push_str(&rest[open + 1..pos]);
            links.push(rest[pos + 2..pos + 2 + close].to_string());
            rest = &rest[pos + 2 + close + 1..];
        } else {
            let Some(gt) = rest[pos..].find('>') else {
                out.push_str(rest);
                break;
            };
            let Some(end) = rest[pos..].find("</a>") else {
                out.push_str(rest);
                break;
            };
            let href = rest[pos + 9..pos + gt].trim_end_matches('"').to_string();
            out.push_str(&rest[..pos]);
            out.push_str(&rest[pos + gt + 1..pos + end]);
            links.push(href);
            rest = &rest[pos + end + 4..];
        }
    }
    (out, links)
}

/// 按差异判定改动类型。`page_exists` 判断站内链接指向的页面是否存在（用构建产物校验）。
/// 只有每一处改动都落在某个已勾选的白名单类型里，才可以自动上线。
pub fn classify(diff: &str, whitelist: &[String], page_exists: &dyn Fn(&str) -> bool) -> Classified {
    let mut kinds: Vec<String> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();
    let add_kind = |k: &str, kinds: &mut Vec<String>| {
        if !kinds.iter().any(|x| x == k) {
            kinds.push(k.to_string());
        }
    };
    let files = parse_diff(diff);
    if files.is_empty() {
        reasons.push("没有任何改动".into());
    }
    for f in &files {
        if f.binary || f.new_file || f.deleted {
            reasons.push(format!("{}：新增 / 删除文件或二进制改动", f.path));
            continue;
        }
        if !CONTENT_EXT.contains(&ext(&f.path)) {
            reasons.push(format!("{}：不是页面内容文件", f.path));
            continue;
        }
        // 逐行配对：改动前后行数一致时按顺序配对；否则只允许纯新增（内链）
        if f.removed.len() == f.added.len() {
            for (old, new) in f.removed.iter().zip(f.added.iter()) {
                if let (Some(_), Some(nv)) = (description_value(old), description_value(new)) {
                    let n = nv.chars().count();
                    if !(30..=170).contains(&n) {
                        reasons.push(format!("{}：描述长度 {n} 不在 30–170 字之间", f.path));
                    }
                    add_kind("meta_description", &mut kinds);
                    continue;
                }
                if let (Some(_), Some(nv)) = (title_value(old), title_value(new)) {
                    let n = nv.chars().count();
                    if n == 0 || n > 70 {
                        reasons.push(format!("{}：标题长度 {n} 不合适", f.path));
                    }
                    add_kind("title", &mut kinds);
                    continue;
                }
                let (old_text, old_links) = strip_internal_links(old);
                let (new_text, new_links) = strip_internal_links(new);
                if old_text == new_text && new_links.len() > old_links.len() && old_links.iter().all(|l| new_links.contains(l)) {
                    for l in new_links.iter().filter(|l| !old_links.contains(l)) {
                        if !page_exists(l) {
                            reasons.push(format!("{}：新链接 {l} 指向的页面不存在", f.path));
                        }
                    }
                    add_kind("internal_links", &mut kinds);
                    continue;
                }
                reasons.push(format!("{}：改动了正文或代码", f.path));
            }
        } else {
            reasons.push(format!("{}：增删了整行内容", f.path));
        }
    }
    for k in &kinds {
        if !whitelist.iter().any(|w| w == k) {
            reasons.push(format!("「{}」没有勾选为可自动上线", kind_label(k)));
        }
    }
    reasons.dedup();
    Classified { kinds, reasons }
}

pub fn kind_label(k: &str) -> &'static str {
    match k {
        "meta_description" => "meta 描述",
        "title" => "标题",
        "internal_links" => "内链",
        "sitemap" => "sitemap 与 IndexNow",
        "meta" => "标题 / 描述",
        "content" => "内容",
        "new_content" => "新内容",
        "technical" => "技术问题",
        "plan" => "AI 计划",
        _ => "其它",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctr_curve_is_monotonic() {
        let mut last = 1.0;
        for p in [1.0, 1.5, 2.0, 3.0, 5.5, 9.0, 10.0, 12.0, 20.0, 50.0] {
            let c = expected_ctr(p);
            assert!(c <= last, "{p}: {c} > {last}");
            last = c;
        }
    }

    fn write(dir: &Path, name: &str, v: serde_json::Value) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), v.to_string()).unwrap();
    }

    fn site() -> Site {
        serde_json::from_value(serde_json::json!({
            "id": "s", "name": "s", "source": "local", "created_at": 0, "updated_at": 0
        }))
        .unwrap()
    }

    #[test]
    fn rules_find_low_ctr_striking_declining_and_gaps() {
        let t = tempfile::tempdir().unwrap();
        let c = cache_dir(t.path());
        let r = |k: &str, k2: &str, clicks: f64, impr: f64, pos: f64| serde_json::json!({"key":k,"key2":k2,"clicks":clicks,"impressions":impr,"ctr":clicks/impr,"position":pos});
        write(&c, "gsc_pages.json", serde_json::json!([
            r("https://x.com/a", "", 2.0, 1000.0, 3.0),     // 点击率 0.2%，排名 3 → low_ctr
            r("https://x.com/b", "", 50.0, 1000.0, 12.0),   // 排名 12 → striking
            r("https://x.com/c", "", 10.0, 500.0, 6.0),     // 点击从 40 掉到 10 → declining
        ]));
        write(&c, "gsc_pages_prev.json", serde_json::json!([r("https://x.com/c", "", 40.0, 600.0, 4.0)]));
        write(&c, "gsc_query_page.json", serde_json::json!([
            r("gate", "https://x.com/a", 2.0, 1000.0, 3.0),
            r("gate fee", "https://x.com/b", 40.0, 800.0, 12.0),
            r("rare", "https://x.com/c", 0.0, 300.0, 35.0),
        ]));
        write(&c, "gsc_queries.json", serde_json::json!([r("rare", "", 0.0, 300.0, 35.0)]));
        let opps = opportunities(&site(), t.path());
        let kinds: Vec<&str> = opps.iter().map(|o| o.kind.as_str()).collect();
        assert!(kinds.contains(&"low_ctr"), "{kinds:?}");
        assert!(kinds.contains(&"striking"));
        assert!(kinds.contains(&"declining"));
        assert!(kinds.contains(&"content_gap"));
        let low = opps.iter().find(|o| o.kind == "low_ctr").unwrap();
        assert_eq!(low.page, "https://x.com/a");
        assert_eq!(low.queries[0].key, "gate");
        assert_eq!(low.change, "meta");
    }

    #[test]
    fn queue_dedupes_and_respects_dismissals() {
        let t = tempfile::tempdir().unwrap();
        let o = Opportunity { key: "low_ctr:a".into(), ..Default::default() };
        assert_eq!(enqueue(t.path(), &[o.clone()], 100, 1000).unwrap().len(), 1);
        assert_eq!(enqueue(t.path(), &[o.clone()], 200, 1000).unwrap().len(), 0, "仍在队列里");
        let id = load_queue(t.path())[0].id.clone();
        update_item(t.path(), &id, 300, |i| i.status = "dismissed".into()).unwrap();
        assert_eq!(enqueue(t.path(), &[o.clone()], 99_999, 1000).unwrap().len(), 0, "驳回过的不再提");
        update_item(t.path(), &id, 300, |i| i.status = "deployed".into()).unwrap();
        assert_eq!(enqueue(t.path(), &[o.clone()], 500, 1000).unwrap().len(), 0, "冷却期内");
        assert_eq!(enqueue(t.path(), &[o], 5000, 1000).unwrap().len(), 1, "冷却期过了可以再提");
    }

    const WL: &[&str] = &["meta_description", "title", "internal_links"];
    fn wl() -> Vec<String> {
        WL.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn description_and_title_edits_are_whitelisted() {
        let diff = "diff --git a/src/content/a.md b/src/content/a.md\nindex 1..2 100644\n--- a/src/content/a.md\n+++ b/src/content/a.md\n@@ -2 +2 @@\n-title: 旧标题\n+title: Gate 邀请码与手续费返佣：2026 最新说明\n@@ -3 +3 @@\n-description: 旧描述\n+description: 用 Gate 邀请码注册可以拿到手续费返佣，这里说明怎么填写、返佣比例和常见问题，以及与其它交易所的对比。\n";
        let c = classify(diff, &wl(), &|_| true);
        assert_eq!(c.kinds, vec!["title".to_string(), "meta_description".to_string()]);
        assert!(c.reasons.is_empty(), "{:?}", c.reasons);
        // 没勾选标题 → 不能自动上线
        let c = classify(diff, &["meta_description".to_string()], &|_| true);
        assert!(c.reasons.iter().any(|r| r.contains("标题")));
    }

    #[test]
    fn short_descriptions_body_edits_and_code_are_rejected() {
        let short = "diff --git a/a.md b/a.md\n--- a/a.md\n+++ b/a.md\n@@ -1 +1 @@\n-description: x\n+description: 太短\n";
        assert!(!classify(short, &wl(), &|_| true).reasons.is_empty());
        let body = "diff --git a/a.md b/a.md\n--- a/a.md\n+++ b/a.md\n@@ -9 +9 @@\n-旧的一段话\n+新的一段话\n";
        assert!(classify(body, &wl(), &|_| true).reasons.iter().any(|r| r.contains("正文")));
        let code = "diff --git a/src/x.ts b/src/x.ts\n--- a/src/x.ts\n+++ b/src/x.ts\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(classify(code, &wl(), &|_| true).reasons.iter().any(|r| r.contains("不是页面内容")));
        let newfile = "diff --git a/n.md b/n.md\nnew file mode 100644\n--- /dev/null\n+++ b/n.md\n@@ -0,0 +1 @@\n+hi\n";
        assert!(!classify(newfile, &wl(), &|_| true).reasons.is_empty());
        let html_meta = "diff --git a/i.html b/i.html\n--- a/i.html\n+++ b/i.html\n@@ -3 +3 @@\n-<meta name=\"description\" content=\"old\">\n+<meta name=\"description\" content=\"一个足够长的描述，说明这个页面讲的是什么内容，以及读者能在这里得到哪些具体帮助和答案。\">\n";
        let c = classify(html_meta, &wl(), &|_| true);
        assert_eq!(c.kinds, vec!["meta_description".to_string()]);
        assert!(c.reasons.is_empty(), "{:?}", c.reasons);
    }

    #[test]
    fn internal_links_must_only_add_links_to_existing_pages() {
        let ok = "diff --git a/a.md b/a.md\n--- a/a.md\n+++ b/a.md\n@@ -5 +5 @@\n-想了解手续费可以看费率说明。\n+想了解手续费可以看[费率说明](/zh/fees/)。\n";
        let c = classify(ok, &wl(), &|p| p == "/zh/fees/");
        assert_eq!(c.kinds, vec!["internal_links".to_string()]);
        assert!(c.reasons.is_empty(), "{:?}", c.reasons);
        let missing = classify(ok, &wl(), &|_| false);
        assert!(missing.reasons.iter().any(|r| r.contains("不存在")));
        let reworded = "diff --git a/a.md b/a.md\n--- a/a.md\n+++ b/a.md\n@@ -5 +5 @@\n-想了解手续费可以看费率说明。\n+手续费请看[这里](/zh/fees/)。\n";
        assert!(classify(reworded, &wl(), &|_| true).reasons.iter().any(|r| r.contains("正文")));
        let html = "diff --git a/a.astro b/a.astro\n--- a/a.astro\n+++ b/a.astro\n@@ -5 +5 @@\n-<p>看费率说明</p>\n+<p>看<a href=\"/zh/fees/\">费率说明</a></p>\n";
        assert!(classify(html, &wl(), &|_| true).reasons.is_empty());
    }
}
