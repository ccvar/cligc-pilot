//! 用 TypeSafe 的判断模型（Jev，见 typesafe.rs）做的两件事：
//! 1. 托管运营自动上线前，给新页面 / 大改的页面做内容质检：没过就不自动上线，留给你确认；
//! 2. 运营机会按「这组搜索词和站点定位的相关度」重新排序，和站点无关的搜索词排到后面。
//!
//! 判断只给数字，阈值和怎么用都在这里的代码里。阈值是初始值，按实际数据再调（质检结果会记在条目上，方便对照）。
//! 没配 Jev、功能关着、调用失败：都按原来的规则走，Jev 只能加严、不会放宽。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

use crate::sites::Site;
use crate::typesafe::{noul, score, Jev};

/// 单个问题的正文上限（字符）：Jev 单个问题 32k token，中文一个字大约一个多 token，留出余量
const PAGE_CHARS: usize = 12_000;

// ---------- 1. 内容质检 ----------

/// 一个页面的质检结果（记在待处理条目上，界面上显示）。分数都是 0–3。
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PageQuality {
    pub page: String,
    /// 有没有把目标问题讲清楚（没有目标查询词时为 None）
    #[serde(default)]
    pub answers: Option<f64>,
    /// 信息量：具体内容多还是空话多
    pub substance: f64,
    /// 和站点定位的相关度
    pub on_topic: f64,
    /// 把具体数字 / 测试结果 / 亲身经历当事实写、却没有来源的概率
    pub unsourced: f64,
    /// 混进写给编辑或 AI 的说明、占位文字的概率
    pub leftover: f64,
    /// 没过的原因（空＝通过）
    #[serde(default)]
    pub reasons: Vec<String>,
}

// 初始阈值：按你的数据再调
const MIN_ANSWERS: f64 = 1.5;
const MIN_SUBSTANCE: f64 = 1.5;
const MIN_ON_TOPIC: f64 = 1.0;
const MAX_UNSOURCED: f64 = 0.75;
const MAX_LEFTOVER: f64 = 0.75;

fn quality_questions(has_target: bool) -> Value {
    let mut q = json!({
        "substance": {
            "type": "score",
            "instructions": "这个页面的正文（`page.text`）里，具体、读者用得上的信息有多少？只看正文本身，不管写得漂不漂亮。",
            "criteria": [
                "大部分是空话、套话或重复的话，几乎找不到具体信息",
                "有少量具体信息，但被大段铺垫和泛泛而谈淹没",
                "大部分段落都有具体内容（步骤、名称、条件、例子、数字），只有少量铺垫",
                "信息密集：几乎每一段都有读者用得上的具体内容，没有凑字数的段落"
            ]
        },
        "on_topic": {
            "type": "score",
            "instructions": "这个页面的主题和站点的定位（`site.positioning`，以及站点名称和域名）有多相关？",
            "criteria": [
                "和站点定位无关，站点的读者不会来看",
                "勉强沾边，但站点的读者一般不会关心这个话题",
                "和站点定位相关，是站点读者会关心的话题",
                "正是站点定位要覆盖的核心话题"
            ]
        },
        "unsourced": {
            "type": "noul",
            "instructions": "页面正文把具体的数字、价格、费率、测试结果、统计数据或亲身经历当作事实来写，却没有说明来源，也没有给出能核对的依据。",
            "criteria": {
                "true": "至少有一处具体数字、测试结果或亲身经历被当成事实陈述，而且看不出出处",
                "false": "没有这类陈述，或者这类陈述都注明了来源、链接或者明确说是举例 / 估算"
            }
        },
        "leftover": {
            "type": "noul",
            "instructions": "页面正文里混进了写给编辑、写给 AI 或者给自己看的话，读者不该看到，比如「此处插入图片」「TODO」「待补充」「作为 AI」「以下是改写后的内容」「根据你的要求」。",
            "criteria": {
                "true": "正文里至少有一处这样的说明、占位或对话口吻",
                "false": "正文全部是写给读者看的内容"
            }
        }
    });
    if has_target {
        q["answers"] = json!({
            "type": "score",
            "instructions": "搜索 `target_query` 的读者看完这个页面（`page.text`），能不能得到他要的答案？",
            "criteria": [
                "页面基本没有回答这个搜索问题，或者答非所问",
                "提到了这个问题，但只有泛泛的介绍，读者看完还是不知道答案或者不知道该怎么做",
                "回答了主要部分，但缺少关键的步骤、条件、例子或对比",
                "完整回答了这个问题：给出明确答案，并有读者需要的步骤、条件、例子或对比"
            ]
        });
    }
    q
}

/// 按阈值判一页：返回没过的原因（给人看的话）。
fn quality_reasons(q: &PageQuality) -> Vec<String> {
    let mut r = Vec::new();
    if let Some(a) = q.answers {
        if a < MIN_ANSWERS {
            r.push(format!("没把目标搜索问题讲清楚（{a:.1}/3）"));
        }
    }
    if q.substance < MIN_SUBSTANCE {
        r.push(format!("空话多、具体信息少（{:.1}/3）", q.substance));
    }
    if q.on_topic < MIN_ON_TOPIC {
        r.push(format!("和站点定位关系不大（{:.1}/3）", q.on_topic));
    }
    if q.unsourced >= MAX_UNSOURCED {
        r.push(format!("有具体数字或测试结果没写来源，可能是编的（{:.0}%）", q.unsourced * 100.0));
    }
    if q.leftover >= MAX_LEFTOVER {
        r.push(format!("正文里像是混进了写给编辑或 AI 的话（{:.0}%）", q.leftover * 100.0));
    }
    r
}

/// 站点信息（定位没写时退回站名和域名）。
fn site_state(site: &Site, positioning: &str) -> Value {
    json!({ "name": site.name, "domain": site.domain, "positioning": positioning })
}

/// 给这次改动里的页面做质检。`app_dir` 是工作树里站点代码的目录，`files` 来自 managed::content_page_files。
/// 每页一次请求（几个问题一起问）；任何一页调用失败就整体返回 Err，调用方按原规则走。
pub async fn check_pages(jev: &Jev, site: &Site, positioning: &str, target: &str, app_dir: &Path, files: &[(String, bool)]) -> Result<Vec<PageQuality>, String> {
    let mut out = Vec::new();
    for (file, _) in files {
        let Ok(src) = std::fs::read_to_string(app_dir.join(file)) else { continue };
        let (title, text) = crate::managed::page_prose(&src, PAGE_CHARS);
        if crate::managed::text_units(&text) < 100 {
            continue; // 几乎没有正文（模板 / 跳转页），不判
        }
        let state = json!({
            "site": site_state(site, positioning),
            "target_query": target,
            "page": { "path": file, "title": title, "text": text },
        });
        let a = jev.ask(state, quality_questions(!target.trim().is_empty())).await?;
        let mut q = PageQuality {
            page: file.clone(),
            answers: if target.trim().is_empty() { None } else { score(&a, "answers") },
            substance: score(&a, "substance").unwrap_or(3.0),
            on_topic: score(&a, "on_topic").unwrap_or(3.0),
            unsourced: noul(&a, "unsourced").unwrap_or(0.0),
            leftover: noul(&a, "leftover").unwrap_or(0.0),
            reasons: Vec::new(),
        };
        q.reasons = quality_reasons(&q);
        out.push(q);
    }
    Ok(out)
}

/// 质检没过的页面合成一句判定（空＝全部通过）。
pub fn gate_verdict(results: &[PageQuality]) -> String {
    results
        .iter()
        .filter(|q| !q.reasons.is_empty())
        .map(|q| format!("{}：{}", q.page, q.reasons.join("、")))
        .collect::<Vec<_>>()
        .join("；")
}

// ---------- 2. 运营机会按相关度排序 ----------

/// 每次最多问多少条（一次请求里每条一个问题，共享同一份站点信息）
const RANK_BATCH: usize = 20;
/// 技术类机会（抓取问题、站点地图出错）和搜索词无关，不打相关度
fn rankable(kind: &str) -> bool {
    !matches!(kind, "crawl_issue" | "sitemap_error")
}

#[derive(Serialize, Deserialize, Default)]
struct CachedRel {
    sig: String,
    rel: f64,
}

/// 这条机会拿去判断的内容：页面 + 搜索词（同样的内容不重复问）
fn opp_sig(o: &crate::site_ops::Opportunity, positioning: &str) -> String {
    use sha2::{Digest, Sha256};
    let qs: Vec<&str> = o.queries.iter().map(|q| q.key.as_str()).collect();
    format!("{:x}", Sha256::digest(format!("{}\n{}\n{}\n{}", positioning, o.page, o.target, qs.join("|")).as_bytes()))[..16].to_string()
}

fn opp_state(o: &crate::site_ops::Opportunity) -> Value {
    let mut queries: Vec<&str> = o.queries.iter().take(8).map(|q| q.key.as_str()).collect();
    if queries.is_empty() && !o.target.is_empty() {
        queries.push(o.target.as_str());
    }
    json!({ "page": o.page, "title": o.title, "queries": queries })
}

/// 按相关度调整分数：完全无关的只剩 1/4，非常相关的保持原样，然后整体重新排序。
fn apply_relevance(opps: &mut [crate::site_ops::Opportunity]) {
    for o in opps.iter_mut() {
        if let Some(rel) = o.relevance {
            o.score *= 0.25 + 0.75 * (rel / 3.0).clamp(0.0, 1.0);
        }
    }
    opps.sort_by(|a, b| b.score.total_cmp(&a.score));
}

/// 给运营机会打相关度并重新排序。结果缓存在站点数据目录（同样的搜索词不重复问）；
/// 站点没写定位、没配 Jev 或调用失败时原样返回（失败的那一批不打分）。
pub async fn rank_opportunities(jev: &Jev, site: &Site, site_dir: &Path, opps: &mut Vec<crate::site_ops::Opportunity>) {
    let positioning = crate::site_stage::load_positioning(site_dir).text;
    if positioning.trim().is_empty() {
        return; // 没有定位就没有「相关」的参照
    }
    let path = crate::site_data::cache_dir(site_dir).join("jev_relevance.json");
    let mut cache: HashMap<String, CachedRel> = crate::store_file::read_or_quarantine(&path);
    let mut todo: Vec<usize> = Vec::new();
    for (i, o) in opps.iter_mut().enumerate() {
        if !rankable(&o.kind) {
            continue;
        }
        let sig = opp_sig(o, &positioning);
        match cache.get(&o.key) {
            Some(c) if c.sig == sig => o.relevance = Some(c.rel),
            _ => todo.push(i),
        }
    }
    for chunk in todo.chunks(RANK_BATCH) {
        let items: Vec<Value> = chunk.iter().map(|&i| opp_state(&opps[i])).collect();
        let mut questions = serde_json::Map::new();
        for n in 0..chunk.len() {
            questions.insert(
                format!("o{n}"),
                json!({
                    "type": "score",
                    "instructions": format!("搜索 `opportunities[{n}].queries` 这些词的人（他们会落到页面 `opportunities[{n}].page`），和这个站点的定位（`site.positioning`）有多相关？"),
                    "criteria": [
                        "无关：这些搜索的人想要的东西和站点没关系（同名词误撞、完全不同的领域、只是想找别的品牌或网站）",
                        "沾点边，但这些搜索的人大多不是这个站点的读者",
                        "相关：这些搜索的人是站点的目标读者，站点能回答他们的问题",
                        "非常相关：这正是站点定位要覆盖的核心需求"
                    ]
                }),
            );
        }
        let state = json!({ "site": site_state(site, &positioning), "opportunities": items });
        let Ok(a) = jev.ask(state, Value::Object(questions)).await else { continue };
        for (n, &i) in chunk.iter().enumerate() {
            if let Some(rel) = score(&a, &format!("o{n}")) {
                opps[i].relevance = Some(rel);
                cache.insert(opps[i].key.clone(), CachedRel { sig: opp_sig(&opps[i], &positioning), rel });
            }
        }
    }
    // 只留现在还在的机会，缓存不无限长大
    let live: std::collections::HashSet<&str> = opps.iter().map(|o| o.key.as_str()).collect();
    cache.retain(|k, _| live.contains(k.as_str()));
    let _ = crate::sites::write_json(&path, &cache, false);
    apply_relevance(opps);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site_ops::Opportunity;

    fn page(answers: Option<f64>, substance: f64, on_topic: f64, unsourced: f64, leftover: f64) -> PageQuality {
        let mut q = PageQuality { page: "src/content/a.md".into(), answers, substance, on_topic, unsourced, leftover, reasons: vec![] };
        q.reasons = quality_reasons(&q);
        q
    }

    #[test]
    fn a_solid_page_passes_and_each_weakness_has_its_own_reason() {
        assert!(page(Some(2.6), 2.4, 2.8, 0.1, 0.02).reasons.is_empty());
        assert!(page(Some(1.0), 2.4, 2.8, 0.1, 0.02).reasons[0].contains("目标搜索问题"));
        assert!(page(None, 1.2, 2.8, 0.1, 0.02).reasons[0].contains("空话多"));
        assert!(page(None, 2.4, 0.4, 0.1, 0.02).reasons[0].contains("站点定位"));
        assert!(page(None, 2.4, 2.8, 0.9, 0.02).reasons[0].contains("没写来源"));
        assert!(page(None, 2.4, 2.8, 0.1, 0.8).reasons[0].contains("写给编辑或 AI"));
        let bad = page(Some(0.8), 1.0, 2.8, 0.1, 0.02);
        assert_eq!(gate_verdict(&[bad.clone(), page(None, 2.5, 2.5, 0.0, 0.0)]), format!("src/content/a.md：{}", bad.reasons.join("、")));
    }

    #[test]
    fn page_prose_keeps_reader_text_and_the_title_only() {
        let src = "---\ntitle: \"如何开户\"\ndescription: x\n---\nimport Hero from '../Hero.astro';\n\n## 第一步\n\n打开<a href=\"/x\">官网</a>，点注册。\n\n```bash\nnpm i\n```\n最后完成验证。\n";
        let (title, body) = crate::managed::page_prose(src, 1000);
        assert_eq!(title, "如何开户");
        assert_eq!(body, "## 第一步\n打开官网，点注册。\n最后完成验证。\n");
        assert_eq!(crate::managed::page_prose(&"字".repeat(50), 10).1.chars().count(), 10);
    }

    #[test]
    fn only_new_pages_and_big_rewrites_get_checked() {
        let long: String = (0..30).map(|i| format!("+这是第{i}段新加的正文内容，讲清楚一个具体的步骤。\n")).collect();
        let diff = format!(
            "diff --git a/src/content/new.md b/src/content/new.md\nnew file mode 100644\n+---\n+title: 新页\n+---\n+短\n\
             diff --git a/src/content/old.md b/src/content/old.md\n{long}\
             diff --git a/src/content/meta.md b/src/content/meta.md\n-description: 旧\n+description: 新\n\
             diff --git a/src/styles/a.css b/src/styles/a.css\nnew file mode 100644\n+body{{}}\n"
        );
        let files = crate::managed::content_page_files(&diff);
        assert_eq!(files, vec![("src/content/new.md".to_string(), true), ("src/content/old.md".to_string(), false)]);
    }

    #[test]
    fn page_without_a_target_query_skips_the_answer_question() {
        assert!(quality_questions(false).get("answers").is_none());
        assert!(quality_questions(true).get("answers").is_some());
    }

    fn opp(key: &str, kind: &str, score: f64, rel: Option<f64>) -> Opportunity {
        Opportunity { key: key.into(), kind: kind.into(), score, relevance: rel, ..Default::default() }
    }

    #[test]
    fn irrelevant_queries_sink_below_smaller_relevant_ones() {
        let mut v = vec![opp("big-but-offtopic", "low_ctr", 100.0, Some(0.0)), opp("small-core", "striking", 60.0, Some(3.0)), opp("crawl", "crawl_issue", 40.0, None)];
        apply_relevance(&mut v);
        assert_eq!(v.iter().map(|o| o.key.as_str()).collect::<Vec<_>>(), vec!["small-core", "crawl", "big-but-offtopic"]);
        assert_eq!(v[2].score, 25.0, "完全无关只剩四分之一");
        assert!(!rankable("sitemap_error") && rankable("content_gap"));
    }

    #[test]
    fn the_same_queries_are_not_asked_twice_but_new_ones_are() {
        let mut a = opp("k", "low_ctr", 1.0, None);
        a.queries = vec![crate::site_data::Row { key: "如何开户".into(), ..Default::default() }];
        let s1 = opp_sig(&a, "数字货币交易所评测");
        assert_eq!(s1, opp_sig(&a, "数字货币交易所评测"));
        a.queries.push(crate::site_data::Row { key: "手续费".into(), ..Default::default() });
        assert_ne!(s1, opp_sig(&a, "数字货币交易所评测"));
        assert_ne!(s1, opp_sig(&opp("k", "low_ctr", 1.0, None), "别的定位"));
    }
}
