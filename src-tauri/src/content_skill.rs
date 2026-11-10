//! 正文不在仓库里、靠仓库自带的内容技能（`skills/*/SKILL.md`、`.claude/skills/*/SKILL.md`）通过管理接口改的站点
//! （blockvar 这类 Go + SQLite 站：文章存在线上数据库里，技能脚本 `get` 取出文档、`put` 校验、`put --apply` 写入）。
//!
//! - 识别：体检时扫一遍技能，有技能且内容变了就让模型读一次，判断正文在不在仓库里、提取「读取 / 校验 / 写入」三条命令。
//!   结果存在站点数据目录的 content_skill.json，要你在站点设置里确认一次才生效。
//! - 派任务：Pilot 先在你的站点文件夹（技能的密钥 .env 在那里，不给 AI）用「读取」把要改的文档取进工作区的
//!   `.pilot/content/`，提交成改动前的样子；AI 只改这些 JSON，不跑技能脚本。
//! - 上线：判定里有 `.pilot/content/` 的改动一律留给你确认；卡片上「试运行」跑校验、「写入线上」跑写入（确认框再问一次）。
//!   命令只用你站点文件夹里本来就有的技能脚本，这次改动新加或改过的脚本一律不跑。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 工作区里放取出来的文档的目录（相对仓库根）
pub const CONTENT_DIR: &str = ".pilot/content";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ContentSkill {
    /// 技能目录（相对仓库根），比如 skills/blockvar-content
    pub skill: String,
    /// 识别时 SKILL.md 的摘要：技能改了就重新识别
    pub sig: String,
    /// 正文在仓库外（数据库 / 管理接口）。false＝正文就在仓库文件里，技能只是写作规范，照常改文件
    pub external: bool,
    /// 取一篇文档存成 JSON：含 {path}（页面路径，如 /en/posts/x/）和 {file}（存到哪）
    pub get: String,
    /// 校验 / 试运行：含 {file}，不会写入线上
    pub check: String,
    /// 写入线上：含 {file}
    pub apply: String,
    /// 一两句话：正文存在哪、技能怎么改、有什么要守的规矩（给你看，也写进给 AI 的说明）
    pub summary: String,
    pub detected_at: u64,
    /// 你在站点设置里确认过（没确认不生效）
    #[serde(default)]
    pub confirmed: bool,
    /// 识别出错时的原因（给你看）
    #[serde(default)]
    pub error: String,
}

impl ContentSkill {
    /// 派任务、上线时按它走
    pub fn active(&self) -> bool {
        self.confirmed && self.external && !self.get.is_empty() && !self.check.is_empty() && !self.apply.is_empty()
    }
}

fn path(site_dir: &Path) -> PathBuf {
    site_dir.join("content_skill.json")
}

pub fn load(site_dir: &Path) -> Option<ContentSkill> {
    let p = path(site_dir);
    p.exists().then(|| crate::store_file::read_or_quarantine(&p))
}

pub fn save(site_dir: &Path, c: &ContentSkill) -> Result<(), String> {
    crate::sites::write_json(&path(site_dir), c, false)
}

/// 站点正在生效的内容技能（确认过、正文在仓库外）
pub fn active(site_dir: &Path) -> Option<ContentSkill> {
    load(site_dir).filter(|c| c.active())
}

/// 仓库里的技能：(目录，SKILL.md 正文)。只看两处：skills/<名字>/ 和 .claude/skills/<名字>/
pub fn find_skills(repo: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for base in ["skills", ".claude/skills"] {
        let Ok(rd) = std::fs::read_dir(repo.join(base)) else { continue };
        let mut dirs: Vec<_> = rd.flatten().filter(|e| e.path().join("SKILL.md").is_file()).collect();
        dirs.sort_by_key(|e| e.file_name());
        for e in dirs {
            if let Ok(t) = std::fs::read_to_string(e.path().join("SKILL.md")) {
                out.push((format!("{base}/{}", e.file_name().to_string_lossy()), t));
            }
        }
    }
    out
}

/// 内容摘要（FNV-1a，跨版本稳定）
pub fn sig_of(skills: &[(String, String)]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (d, t) in skills {
        for b in d.bytes().chain([0u8]).chain(t.bytes()).chain([0u8]) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// 识别用的提示词
fn detect_prompt(skills: &[(String, String)]) -> String {
    let body: String = skills
        .iter()
        .take(4)
        .map(|(d, t)| format!("=== 技能目录：{d}（SKILL.md）===\n{}\n", t.chars().take(12_000).collect::<String>()))
        .collect();
    format!(
        "下面是一个网站代码仓库里自带的技能说明。请判断：这个网站的文章正文是存在仓库文件里，还是存在仓库外（线上数据库、管理接口），要用技能里的脚本去读写？\n\
如果正文在仓库外，从说明里找出三条在仓库根目录运行的命令：\n\
- get：取出一篇已有文档存成本地 JSON 文件，用 {{path}} 代表页面路径（如 /en/posts/x/），{{file}} 代表存到的文件；\n\
- check：校验这份 JSON，不写入线上（试运行），用 {{file}} 代表文件；\n\
- apply：把这份 JSON 写入线上，用 {{file}} 代表文件。\n\
命令必须直接调用技能目录里已有的脚本（比如 python3 skills/x/scripts/x.py …），不要管道、重定向、&&、环境变量，不要编造说明里没有的参数。\n\
只输出一个 JSON 对象，不要别的文字：\n\
{{\"skill\": \"技能目录\", \"external\": true 或 false, \"get\": \"…\", \"check\": \"…\", \"apply\": \"…\", \"summary\": \"一两句中文：正文存在哪、用哪个脚本怎么改、要守的规矩（比如新文档先草稿、推荐链接不进正文）\"}}\n\
正文就在仓库文件里时 external 写 false，三条命令写空字符串；有几个技能时选管内容的那个。\n\n{body}"
    )
}

#[derive(Deserialize, Default)]
struct Detected {
    #[serde(default)]
    skill: String,
    #[serde(default)]
    external: bool,
    #[serde(default)]
    get: String,
    #[serde(default)]
    check: String,
    #[serde(default)]
    apply: String,
    #[serde(default)]
    summary: String,
}

/// 从模型回复里取出 JSON 对象
fn parse_detected(raw: &str) -> Option<Detected> {
    let s = raw.find('{')?;
    let e = raw.rfind('}')?;
    serde_json::from_str(raw.get(s..=e)?).ok()
}

/// 命令的安全检查：只能是「解释器 + 技能目录里已有的脚本 + 参数」或者直接调用技能目录里的脚本，不能有 shell 语法。
/// `need`：必须含的占位符。返回拆好的参数（占位符原样保留）。
pub fn validate_cmd(repo: &Path, skill: &str, cmd: &str, need: &[&str]) -> Result<Vec<String>, String> {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return Err("命令是空的".into());
    }
    if cmd.chars().any(|c| matches!(c, ';' | '&' | '|' | '`' | '$' | '<' | '>' | '\n' | '\\' | '"' | '\'' | '*' | '?' | '(' | ')')) {
        return Err(format!("命令里不能有 shell 语法：{cmd}"));
    }
    let argv: Vec<String> = cmd.split_whitespace().map(String::from).collect();
    for n in need {
        if !argv.iter().any(|a| a.contains(n)) {
            return Err(format!("命令里少了 {n}：{cmd}"));
        }
    }
    let script_at = if matches!(argv[0].as_str(), "python3" | "python" | "node" | "bash" | "sh") { 1 } else { 0 };
    let script = argv.get(script_at).ok_or("命令里没有脚本")?;
    let skill = skill.trim_end_matches('/');
    if skill.is_empty() || skill.contains("..") || script.contains("..") || script.starts_with('/') || !script.starts_with(&format!("{skill}/")) {
        return Err(format!("命令要调用技能目录 {skill} 里的脚本：{cmd}"));
    }
    if !repo.join(script).is_file() {
        return Err(format!("站点文件夹里没有这个脚本：{script}"));
    }
    Ok(argv)
}

/// 识别一次：读技能、问模型、检查命令。`brain` / `model` 用站点运营的那一家（小模型）。
pub async fn detect(data_dir: &Path, repo: &Path, brain: &str, model: &str) -> ContentSkill {
    let skills = find_skills(repo);
    let sig = sig_of(&skills);
    let mut c = ContentSkill { sig, detected_at: crate::now_secs(), ..Default::default() };
    if skills.is_empty() {
        return c;
    }
    let prompt = detect_prompt(&skills);
    // 先用本机 Claude 客户端的小模型；没装就用站点运营的 API 连接的小模型
    let local = crate::oneshot::Ask { brain: "claude", model: "", prompt: &prompt, max_tokens: 800, small: true, timeout: Duration::from_secs(120) };
    let mut raw = crate::oneshot::complete(data_dir, &local).await;
    if raw.is_err() && brain.starts_with("api:") {
        let api = crate::oneshot::Ask { brain, model, prompt: &prompt, max_tokens: 800, small: true, timeout: Duration::from_secs(120) };
        raw = crate::oneshot::complete(data_dir, &api).await;
    }
    let raw = match raw {
        Ok(r) => r,
        Err(e) => {
            c.error = format!("识别没做成：{e}");
            return c;
        }
    };
    let Some(d) = parse_detected(&raw) else {
        c.error = "识别没做成：模型的回复看不懂".into();
        return c;
    };
    c.skill = d.skill.trim().trim_end_matches('/').to_string();
    if c.skill.is_empty() || !skills.iter().any(|(s, _)| *s == c.skill) {
        c.skill = skills[0].0.clone();
    }
    c.external = d.external;
    c.summary = d.summary.trim().chars().take(400).collect();
    if !c.external {
        return c;
    }
    let checks = [(&d.get, &["{path}", "{file}"][..], "读取"), (&d.check, &["{file}"][..], "校验"), (&d.apply, &["{file}"][..], "写入")];
    for (cmd, need, label) in checks {
        if let Err(e) = validate_cmd(repo, &c.skill, cmd, need) {
            c.error = format!("{label}命令不能用：{e}");
        }
    }
    if d.check.trim() == d.apply.trim() {
        c.error = "校验和写入是同一条命令，分不出试运行".into();
    }
    if c.error.is_empty() {
        c.get = d.get.trim().to_string();
        c.check = d.check.trim().to_string();
        c.apply = d.apply.trim().to_string();
    }
    c
}

/// 体检时调：技能有变化（或第一次见到）就重新识别。命令和判断都没变的，保留你之前的确认。返回要写进作业日志的一句话。
pub async fn refresh(site_dir: &Path, data_dir: &Path, repo: &Path, brain: &str, model: &str, force: bool) -> Option<String> {
    let skills = find_skills(repo);
    let saved = load(site_dir);
    if skills.is_empty() {
        if saved.is_some() {
            let _ = std::fs::remove_file(path(site_dir));
            return Some("仓库里的内容技能没有了，按正文在仓库里处理".into());
        }
        return None;
    }
    let sig = sig_of(&skills);
    if !force && saved.as_ref().is_some_and(|s| s.sig == sig && s.error.is_empty()) {
        return None;
    }
    let mut c = detect(data_dir, repo, brain, model).await;
    if let Some(s) = &saved {
        c.confirmed = s.confirmed && s.external == c.external && s.skill == c.skill && s.get == c.get && s.check == c.check && s.apply == c.apply;
    }
    let _ = save(site_dir, &c);
    Some(if !c.error.is_empty() {
        format!("仓库里有内容技能 {}，{}", c.skill, c.error)
    } else if c.external && !c.confirmed {
        format!("识别到内容技能 {}：正文在仓库外（{}）。到站点设置里确认后，运营任务按它改", c.skill, c.summary)
    } else if c.external {
        format!("内容技能 {} 有更新，命令没变，照旧按它改", c.skill)
    } else {
        format!("仓库里有技能 {}，正文就在仓库文件里，照常改文件", c.skill)
    })
}

/// 一次运行的结果
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub at: u64,
    /// check | apply | get
    pub kind: String,
    /// 哪份文档（相对仓库根）
    pub file: String,
    /// 跑的时候分支在哪个提交（试运行要和写入时是同一版）
    #[serde(default)]
    pub head: String,
    pub ok: bool,
    pub output: String,
}

/// 在站点文件夹里跑一条技能命令（不经过 shell，占位符按参数替换）。返回 (成功，输出)。
pub async fn run(repo: &Path, skill: &str, template: &str, need: &[&str], vars: &[(&str, &str)]) -> (bool, String) {
    let argv = match validate_cmd(repo, skill, template, need) {
        Ok(a) => a,
        Err(e) => return (false, e),
    };
    let argv: Vec<String> = argv.iter().map(|a| vars.iter().fold(a.clone(), |s, (k, v)| s.replace(k, v))).collect();
    let mut cmd = tokio::process::Command::new(&argv[0]);
    cmd.args(&argv[1..]).current_dir(repo).kill_on_drop(true).stdin(std::process::Stdio::null());
    match tokio::time::timeout(Duration::from_secs(180), cmd.output()).await {
        Err(_) => (false, "3 分钟没跑完，已停止".into()),
        Ok(Err(e)) => (false, format!("运行不了：{e}")),
        Ok(Ok(o)) => {
            let mut out = String::from_utf8_lossy(&o.stdout).to_string();
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.trim().is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&err);
            }
            let out: String = out.trim().chars().take(8000).collect();
            (o.status.success(), out)
        }
    }
}

/// 页面路径 → 工作区里的文件名（/en/posts/what-is-x/ → en-posts-what-is-x.json）
pub fn file_for(path: &str) -> String {
    let s = crate::site_git::slugify(&path.replace('/', " "), 80);
    format!("{CONTENT_DIR}/{}.json", if s.is_empty() { "home".to_string() } else { s })
}

/// 改动里动到的文档（相对仓库根），删掉的不算
pub fn changed_docs(files: &[String]) -> Vec<String> {
    files.iter().filter(|f| f.starts_with(&format!("{CONTENT_DIR}/")) && f.ends_with(".json")).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试一个目录：测试并行跑，共用一个会被别的测试删掉
    fn repo(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pilot-content-skill-{name}-{}", std::process::id()));
        std::fs::create_dir_all(d.join("skills/blockvar-content/scripts")).unwrap();
        std::fs::write(d.join("skills/blockvar-content/scripts/blockvar.py"), "import sys\nprint('ok', ' '.join(sys.argv[1:]))\nsys.exit(1 if '--fail' in sys.argv else 0)\n").unwrap();
        std::fs::write(d.join("skills/blockvar-content/SKILL.md"), "---\nname: blockvar-content\n---\nput then put --apply").unwrap();
        d
    }

    #[test]
    fn commands_must_call_an_existing_skill_script_without_shell_syntax() {
        let r = repo("commands_mus");
        let s = "skills/blockvar-content";
        assert!(validate_cmd(&r, s, "python3 skills/blockvar-content/scripts/blockvar.py put {file} --apply", &["{file}"]).is_ok());
        assert!(validate_cmd(&r, s, "python3 skills/blockvar-content/scripts/blockvar.py get {path} --output {file}", &["{path}", "{file}"]).is_ok());
        // 少占位符、shell 语法、技能目录外的脚本、不存在的脚本、跳出目录：都不行
        assert!(validate_cmd(&r, s, "python3 skills/blockvar-content/scripts/blockvar.py put", &["{file}"]).is_err());
        assert!(validate_cmd(&r, s, "python3 skills/blockvar-content/scripts/blockvar.py put {file}; rm -rf /", &["{file}"]).is_err());
        assert!(validate_cmd(&r, s, "python3 scripts/patch.py {file}", &["{file}"]).is_err());
        assert!(validate_cmd(&r, s, "python3 skills/blockvar-content/scripts/new.py {file}", &["{file}"]).is_err());
        assert!(validate_cmd(&r, s, "python3 skills/blockvar-content/../../x.py {file}", &["{file}"]).is_err());
        assert!(validate_cmd(&r, s, "curl https://x {file}", &["{file}"]).is_err());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn runs_with_placeholders_as_plain_arguments() {
        let r = repo("runs_with_pl");
        let s = "skills/blockvar-content";
        let t = tokio::runtime::Runtime::new().unwrap();
        let (ok, out) = t.block_on(run(&r, s, "python3 skills/blockvar-content/scripts/blockvar.py put {file}", &["{file}"], &[("{file}", "/tmp/a b;rm.json")]));
        assert!(ok, "{out}");
        assert_eq!(out, "ok put /tmp/a b;rm.json", "带空格、分号的路径原样当一个参数传进去，不经过 shell");
        let (ok, _) = t.block_on(run(&r, s, "python3 skills/blockvar-content/scripts/blockvar.py put {file} --fail", &["{file}"], &[("{file}", "x")]));
        assert!(!ok);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn finds_skills_and_parses_the_models_answer() {
        let r = repo("finds_skills");
        let found = find_skills(&r);
        assert_eq!(found.iter().map(|(d, _)| d.as_str()).collect::<Vec<_>>(), vec!["skills/blockvar-content"]);
        assert_ne!(sig_of(&found), sig_of(&[]));
        let d = parse_detected("好的：\n{\"skill\":\"skills/blockvar-content\",\"external\":true,\"get\":\"a\",\"check\":\"b\",\"apply\":\"c\",\"summary\":\"正文在 SQLite\"}\n").unwrap();
        assert!(d.external && d.summary == "正文在 SQLite");
        assert_eq!(file_for("/en/posts/what-is-testnet-faucet-guide/"), ".pilot/content/en-posts-what-is-testnet-faucet-guide.json");
        assert_eq!(changed_docs(&[".pilot/content/a.json".into(), ".pilot/site.md".into(), "x.go".into()]), vec![".pilot/content/a.json".to_string()]);
        let _ = std::fs::remove_dir_all(&r);
    }
}

/// 真实站点上跑一次识别 + 取文档（只读）：
/// CONTENT_REPO=~/work/blockvar.com CONTENT_PATH=/en/posts/x/ cargo test --lib content_skill::live -- --ignored --nocapture
#[cfg(test)]
mod live {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn detect_and_get_real() {
        let repo = PathBuf::from(std::env::var("CONTENT_REPO").expect("CONTENT_REPO"));
        let data = std::env::temp_dir();
        let c = detect(&data, &repo, "claude", "").await;
        println!("{c:#?}");
        assert!(c.error.is_empty(), "{}", c.error);
        if let (true, Ok(p)) = (c.external, std::env::var("CONTENT_PATH")) {
            let out = std::env::temp_dir().join("pilot-content-live.json");
            let (ok, log) = run(&repo, &c.skill, &c.get, &["{path}", "{file}"], &[("{path}", &p), ("{file}", &out.to_string_lossy())]).await;
            println!("get ok={ok}\n{log}");
            assert!(ok && out.is_file());
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
            println!("title = {}", v["title"]);
            let _ = std::fs::remove_file(&out);
        }
    }
}
