//! 站点阶段与站点定位：托管运营按阶段决定本周做什么，按定位推导增量选题。
//!
//! - 阶段：新站（没有或很少搜索数据）/ 成长期（有展现、点击还少）/ 成熟期（稳定流量），
//!   按 Search Console 近 28 天自动判断；运营人员可以在站点「运营」里手动指定。
//! - 定位：AI 每次做计划前读站点归纳一段（主题、读者、语言、栏目、内容类型、商业目标），存在站点数据目录；
//!   运营人员改过就以运营人员的为准，AI 不再覆盖。大部分运营不知道该写什么，选题就从这里推导。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::sites::Site;

pub const STAGES: &[&str] = &["new", "growing", "established"];

pub fn label(stage: &str) -> &'static str {
    match stage {
        "new" => "新站",
        "growing" => "成长期",
        "established" => "成熟期",
        _ => "未知",
    }
}

/// 按数据自动判断的阶段和理由。
pub fn auto_stage(site: &Site, site_dir: &Path) -> (&'static str, String) {
    if site.data.gsc_site.is_empty() {
        return ("new", "还没有接入 Search Console".into());
    }
    let d = crate::site_data::dashboard(site_dir);
    if d.meta.gsc.fetched_at == 0 {
        return ("new", "Search Console 还没拉到数据".into());
    }
    let (clicks, impr) = (d.gsc_totals.clicks, d.gsc_totals.impressions);
    if impr < 200.0 {
        ("new", format!("近 28 天展现 {impr:.0}，搜索数据还很少"))
    } else if clicks < 200.0 {
        ("growing", format!("近 28 天展现 {impr:.0}、点击 {clicks:.0}：有曝光，点击还少"))
    } else {
        ("established", format!("近 28 天点击 {clicks:.0}、展现 {impr:.0}：已有稳定流量"))
    }
}

#[derive(Serialize, Clone)]
pub struct StageInfo {
    pub stage: String,
    pub auto_stage: String,
    pub reason: String,
    /// 运营人员手动指定的阶段（空＝自动）。
    pub manual: String,
}

pub fn stage_of(site: &Site, site_dir: &Path) -> StageInfo {
    let (auto, reason) = auto_stage(site, site_dir);
    let manual = if STAGES.contains(&site.ops.stage.as_str()) { site.ops.stage.clone() } else { String::new() };
    StageInfo { stage: if manual.is_empty() { auto.to_string() } else { manual.clone() }, auto_stage: auto.into(), reason, manual }
}

/// 这个阶段本周该做什么（写进计划指令）。
pub fn guidance(stage: &str) -> &'static str {
    match stage {
        "new" => "这个站点的搜索数据还很少（可能是新站，也可能是已有内容、但还没被搜索引擎收录和排上去）。不要编造流量数字，零星的查询数据只作参考。本周重点：\n\
① 下面体检发现的技术问题；\n\
② 已有不少页面的话，先更新过时的内容（价格、版本、步骤），把薄页扩成完整的页面；\n\
③ 按站点定位做增量内容：先补齐核心主题的支柱页和目标读者最常问的问题，再做对比、教程、术语解释这类长尾页面，每页只回应一个明确的搜索意图；\n\
④ 新页面之间、新旧页面之间的站内链接。",
        "growing" => "这个站点有一些展现，但点击还少。本周重点：\n\
① 排名 8–20、有展现没点击的查询词：补对应页面的内容，或改标题和描述；\n\
② 有搜索需求但没有专门页面的内容缺口；\n\
③ 在此基础上按站点定位做增量内容，优先和已有展现的主题相邻的选题。",
        _ => "这个站点已有稳定流量。本周以 SEO 调整为主：\n\
① 点击率偏低的页面改标题和描述；\n\
② 点击下滑的页面更新内容；\n\
③ 接近首页（排名 4–15）的页面加强内容和站内链接；\n\
④ 增量内容只占少数，只做和已有流量主题紧密相关、数据显示有需求的选题。",
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Positioning {
    pub text: String,
    /// ai：AI 归纳的，下次计划可以更新；user：运营人员改过的，AI 照用、不覆盖。
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub updated: u64,
}

fn pos_file(site_dir: &Path) -> PathBuf {
    site_dir.join("positioning.json")
}

pub fn load_positioning(site_dir: &Path) -> Positioning {
    std::fs::read(pos_file(site_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_positioning(site_dir: &Path, p: &Positioning) -> Result<(), String> {
    std::fs::create_dir_all(site_dir).map_err(|e| e.to_string())?;
    std::fs::write(pos_file(site_dir), serde_json::to_vec_pretty(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// AI 在计划里交回的定位：运营人员改过的不覆盖。
pub fn update_from_ai(site_dir: &Path, text: &str) {
    let text = text.trim();
    if text.is_empty() || load_positioning(site_dir).by == "user" {
        return;
    }
    let _ = save_positioning(site_dir, &Positioning { text: text.chars().take(1200).collect(), by: "ai".into(), updated: crate::now_secs() });
}

/// 计划指令里的「站点阶段 + 站点定位 + 选题来源」一段。
pub fn plan_brief(site: &Site, site_dir: &Path, has_topics: bool) -> String {
    let st = stage_of(site, site_dir);
    let pos = load_positioning(site_dir);
    let pos_part = if pos.text.trim().is_empty() {
        "（还没有。请先读站点的首页、导航和主要栏目，归纳出定位再做计划。）".to_string()
    } else if pos.by == "user" {
        format!("{}\n（这是运营人员确认过的定位，按它来，不要改写。）", pos.text.trim())
    } else {
        format!("{}\n（这是你上次归纳的；读站点后如果发现不准确，在输出里更新。）", pos.text.trim())
    };
    format!(
        "【站点阶段】{}（{}{}）\n{}\n\n【站点定位】\n{}\n\n【选题来源】按优先级：{}数据里的内容缺口 → 根据站点定位推导的增量选题。推导时想清楚：定位里还有哪些主题、哪些读者问题没有被现有页面覆盖，目标读者会怎么搜。每个新选题在 why 里写清它补的是定位里的哪一块、预计的目标查询词；不要和已有页面重复（先看站点现有的页面）。\n\n",
        label(&st.stage),
        st.reason,
        if st.manual.is_empty() { "，自动判断" } else { "；运营人员手动指定了这个阶段" },
        guidance(&st.stage),
        pos_part,
        if has_topics { "选题清单 → " } else { "" },
    )
}

/// 最近一次体检报告（截断），新站阶段写进计划指令。
pub fn latest_health(site_dir: &Path, max_chars: usize) -> String {
    let d = crate::site_runner::reports_dir(site_dir);
    let mut names: Vec<String> = std::fs::read_dir(&d).ok().into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with("-health.md")).collect();
    names.sort();
    names.last().and_then(|n| std::fs::read_to_string(d.join(n)).ok()).map(|t| t.chars().take(max_chars).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> Site {
        serde_json::from_value(serde_json::json!({ "id": "s", "name": "x.com", "domain": "x.com", "source": "local", "local_dir": "/tmp", "created_at": 0, "updated_at": 0 })).unwrap()
    }

    #[test]
    fn sites_without_search_console_are_new_and_manual_stage_wins() {
        let t = tempfile::tempdir().unwrap();
        let mut s = site();
        let st = stage_of(&s, t.path());
        assert_eq!((st.stage.as_str(), st.manual.as_str()), ("new", ""));
        s.ops.stage = "established".into();
        let st = stage_of(&s, t.path());
        assert_eq!((st.stage.as_str(), st.auto_stage.as_str()), ("established", "new"));
        s.ops.stage = "bogus".into();
        assert_eq!(stage_of(&s, t.path()).stage, "new");
    }

    #[test]
    fn ai_never_overwrites_a_positioning_the_operator_edited() {
        let t = tempfile::tempdir().unwrap();
        update_from_ai(t.path(), "AI 版本");
        assert_eq!(load_positioning(t.path()).by, "ai");
        save_positioning(t.path(), &Positioning { text: "运营版本".into(), by: "user".into(), updated: 1 }).unwrap();
        update_from_ai(t.path(), "AI 新版本");
        assert_eq!(load_positioning(t.path()).text, "运营版本");
        let b = plan_brief(&site(), t.path(), false);
        assert!(b.contains("运营版本") && b.contains("不要改写") && b.contains("【站点阶段】新站"));
        assert!(!b.contains("选题清单 →"));
    }
}
