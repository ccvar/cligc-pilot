//! 本地用量日志：每轮 token 追加一行 JSONL（usage_log.jsonl），供「近 5 小时 / 今日」
//! 参考统计。只做本地体感计量——订阅额度官方无查询接口，换算不了真实剩余百分比。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub struct UsageEntry {
    pub ts: i64, // unix 秒
    pub brain: String,
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_create: u64,
    /// 补记的老数据：只知道会话的折算 token（记在 input 里），没有输入 / 输出 / 缓存的拆分，金额是粗估
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rough: bool,
}

fn log_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("usage_log.jsonl")
}

/// 追加一轮用量。文件超 1MB 时顺手裁剪到最近 8 天（重写）。追加失败静默——
/// 参考统计不值得让回合报错。
pub fn append(data_dir: &Path, e: &UsageEntry) {
    let p = log_path(data_dir);
    let Ok(line) = serde_json::to_string(e) else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let _ = writeln!(f, "{line}");
    }
    if std::fs::metadata(&p)
        .map(|m| m.len() > 1_000_000)
        .unwrap_or(false)
    {
        prune(&p, e.ts - 8 * 24 * 3600);
    }
}

fn prune(p: &Path, keep_since: i64) {
    let Ok(raw) = std::fs::read_to_string(p) else {
        return;
    };
    let kept: Vec<&str> = raw
        .lines()
        .filter(|l| {
            serde_json::from_str::<UsageEntry>(l)
                .map(|e| e.ts >= keep_since)
                .unwrap_or(false)
        })
        .collect();
    let tmp = p.with_extension("jsonl.tmp");
    if std::fs::write(&tmp, kept.join("\n") + "\n").is_ok() {
        let _ = std::fs::rename(&tmp, p);
    }
}

// ───────────────────────── 每个站点的 AI 花费 ─────────────────────────
// 运营作业（托管、站点运营）开出来的会话，每一轮的用量另记一份到站点数据目录的 ai_usage.jsonl，
// 不跟上面那份 8 天就裁掉的全局记录走：要按月看每个站点花了多少。

fn site_log(site_dir: &Path) -> std::path::PathBuf {
    site_dir.join("ai_usage.jsonl")
}

pub fn append_site(site_dir: &Path, e: &UsageEntry) {
    let p = site_log(site_dir);
    let Ok(line) = serde_json::to_string(e) else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{line}");
    }
    if std::fs::metadata(&p).map(|m| m.len() > 2_000_000).unwrap_or(false) {
        prune(&p, e.ts - 400 * 24 * 3600);
    }
}

/// 第一次看花费时，站点还没有自己的用量记录：用还在的运营会话补一份（只有折算 token，标成粗估）。
pub fn backfill_site(site_dir: &Path, convs: &[&crate::convo::Conversation]) {
    let p = site_log(site_dir);
    if p.exists() {
        return;
    }
    let mut lines = String::new();
    for c in convs {
        let t = if c.billed_tokens > 0 { c.billed_tokens } else { c.total_tokens };
        if t == 0 {
            continue;
        }
        let e = UsageEntry { ts: c.updated_at as i64, brain: c.brain.clone(), model: c.model.clone(), input: t, output: 0, cache_read: 0, cache_create: 0, rough: true };
        if let Ok(l) = serde_json::to_string(&e) {
            lines.push_str(&l);
            lines.push('\n');
        }
    }
    let _ = std::fs::write(&p, lines);
}

/// 官方价（美元 / 百万 token）：输入、输出、缓存读取、缓存写入（5 分钟）。只认 Claude 的型号，认不出的不估金额。
fn price(model: &str) -> Option<(f64, f64, f64, f64)> {
    let m = model.to_ascii_lowercase();
    let has = |k: &str| m.contains(k);
    Some(if has("fable-5-1") || has("mythos-5-1") {
        (10.0, 50.0, 0.25, 12.5)
    } else if has("fable") || has("mythos") {
        (10.0, 50.0, 1.0, 12.5)
    } else if has("opus-5-5") {
        (4.0, 20.0, 0.2, 5.0)
    } else if has("opus-5") || has("opus-4-8") || has("opus-4-7") || has("opus-4-6") || has("opus-4-5") {
        (5.0, 25.0, 0.5, 6.25)
    } else if has("sonnet-5") {
        (2.0, 10.0, 0.2, 2.5)
    } else if has("sonnet-4") {
        (3.0, 15.0, 0.3, 3.75)
    } else if has("haiku-5") {
        (0.1, 0.5, 0.01, 0.125)
    } else if has("haiku-4") {
        (1.0, 5.0, 0.1, 1.25)
    } else {
        return None;
    })
}

/// 这一轮按官方价大约多少钱。只有 API 连接按 token 计费；Claude Code、Codex 等走订阅额度，不算钱（返回 None）。
pub fn usd(e: &UsageEntry) -> Option<f64> {
    if !e.brain.starts_with("api:") {
        return None;
    }
    let (i, o, cr, cw) = price(&e.model)?;
    Some((e.input as f64 * i + e.output as f64 * o + e.cache_read as f64 * cr + e.cache_create as f64 * cw) / 1e6)
}

/// 折算 token（缓存读取按 1/10、缓存写入按 1.25 倍）：和运营预算同一口径
fn billed(e: &UsageEntry) -> u64 {
    e.input + e.output + e.cache_create + e.cache_create / 4 + e.cache_read / 10
}

#[derive(Serialize, Default, Clone, Debug, PartialEq)]
pub struct Cost {
    /// 折算 token
    pub tokens: u64,
    /// 能估出金额的那部分（API 连接、认得出的型号）合计多少美元
    pub usd: f64,
    /// 走订阅额度、不按 token 计费的部分（Claude Code、Codex…）
    pub sub_tokens: u64,
    /// 按 token 计费、但认不出型号价格的部分
    pub unpriced_tokens: u64,
    /// 里面有补记的粗估数据
    pub rough: bool,
    /// 模型回合数
    pub turns: u32,
}

impl Cost {
    pub fn add(&mut self, o: &Cost) {
        self.tokens += o.tokens;
        self.usd += o.usd;
        self.sub_tokens += o.sub_tokens;
        self.unpriced_tokens += o.unpriced_tokens;
        self.rough |= o.rough;
        self.turns += o.turns;
    }
}

/// 某个站点从 since 起的 AI 用量
pub fn site_cost(site_dir: &Path, since: i64) -> Cost {
    let mut c = Cost::default();
    let Ok(raw) = std::fs::read_to_string(site_log(site_dir)) else { return c };
    for l in raw.lines() {
        let Ok(e) = serde_json::from_str::<UsageEntry>(l) else { continue };
        if e.ts < since {
            continue;
        }
        let t = billed(&e);
        c.tokens += t;
        c.turns += 1;
        c.rough |= e.rough;
        match usd(&e) {
            Some(v) => c.usd += v,
            None if e.brain.starts_with("api:") => c.unpriced_tokens += t,
            None => c.sub_tokens += t,
        }
    }
    c
}

/// 两个起点各算一份「按 brain 汇总的处理量」（input+cache_read+cache_create+output，
/// 与会话累计 total_tokens 同口径）。起点由前端按本地时区算好传来（近 5 小时 / 今日零点）。
#[derive(Serialize, Default)]
pub struct UsageStats {
    pub window_a: HashMap<String, u64>,
    pub window_b: HashMap<String, u64>,
}

pub fn stats(data_dir: &Path, since_a: i64, since_b: i64) -> UsageStats {
    let mut out = UsageStats::default();
    let Ok(raw) = std::fs::read_to_string(log_path(data_dir)) else {
        return out;
    };
    for l in raw.lines() {
        let Ok(e) = serde_json::from_str::<UsageEntry>(l) else {
            continue;
        };
        let total = e.input + e.cache_read + e.cache_create + e.output;
        if e.ts >= since_a {
            *out.window_a.entry(e.brain.clone()).or_insert(0) += total;
        }
        if e.ts >= since_b {
            *out.window_b.entry(e.brain).or_insert(0) += total;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_stats_windows() {
        let dir =
            std::env::temp_dir().join(format!("pilot-usage-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mk = |ts: i64, brain: &str, output: u64| UsageEntry {
            ts,
            brain: brain.into(),
            model: "m".into(),
            input: 100,
            output,
            cache_read: 50,
            cache_create: 0,
            rough: false,
        };
        append(&dir, &mk(1000, "claude", 10)); // 只落在窗口 B
        append(&dir, &mk(5000, "claude", 20)); // 两个窗口都在
        append(&dir, &mk(6000, "codex", 30));
        let s = stats(&dir, 4000, 500);
        assert_eq!(s.window_a.get("claude"), Some(&170u64)); // 100+50+20
        assert_eq!(s.window_a.get("codex"), Some(&180u64));
        assert_eq!(s.window_b.get("claude"), Some(&(160u64 + 170)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn site_cost_prices_api_turns_and_counts_subscriptions_apart() {
        let dir = tempfile::tempdir().unwrap();
        let e = |ts: i64, brain: &str, model: &str| UsageEntry { ts, brain: brain.into(), model: model.into(), input: 1_000_000, output: 100_000, cache_read: 10_000_000, cache_create: 0, rough: false };
        append_site(dir.path(), &e(10, "api:x", "claude-opus-5-5")); // 起点之前，不算
        append_site(dir.path(), &e(100, "api:x", "claude-opus-5-5")); // 4 + 2 + 2 = $8
        append_site(dir.path(), &e(100, "claude", "claude-sonnet-5")); // 订阅
        append_site(dir.path(), &e(100, "api:y", "deepseek-chat")); // 认不出价格
        let c = site_cost(dir.path(), 50);
        assert_eq!(c.turns, 3);
        assert!((c.usd - 8.0).abs() < 1e-9, "{}", c.usd);
        assert_eq!(c.sub_tokens, 2_100_000);
        assert_eq!(c.unpriced_tokens, 2_100_000);
        assert_eq!(c.tokens, 3 * 2_100_000);
    }
}
