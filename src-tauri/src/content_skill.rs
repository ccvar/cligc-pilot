//! 正文不在仓库里、靠仓库自带的内容技能（`skills/*/SKILL.md`、`.claude/skills/*/SKILL.md`）通过管理接口改的站点
//! （blockvar 这类 Go + SQLite 站：文章存在线上数据库里，技能脚本 `get` 取出文档、`put` 校验、`put --apply` 写入）。
//!
//! - 识别：体检时扫一遍技能，有技能且内容变了就让模型读一次，判断正文在不在仓库里、提取「读取 / 校验 / 写入」三条命令。
//!   结果存在站点数据目录的 content_skill.json，要你在站点设置里确认一次才生效。
//! - 派任务：Pilot 先在你的站点文件夹（技能的密钥 .env 在那里，不给 AI）用「读取」把要改的文档取进工作区的
//!   `.pilot/content/`，提交成改动前的样子；AI 只改这些 JSON，不跑技能脚本。
//! - 上线：判定里有 `.pilot/content/` 的改动一律留给你确认；卡片上「试运行」跑校验、「写入线上」跑写入（确认框再问一次）。
//!   命令只用你站点文件夹里本来就有的技能脚本，这次改动新加或改过的脚本一律不跑。
//! - 配图：AI 截的图放在 `.pilot/media/`，文档里直接写这个相对路径；试运行 / 写入前 Pilot 用技能的「上传」命令
//!   把图传上去，换成返回的线上地址再交给校验和写入（工作区里的文档不动）。技能没有上传命令时，引用了图片的文档不写入。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 工作区里放取出来的文档的目录（相对仓库根）
pub const CONTENT_DIR: &str = ".pilot/content";
/// 工作区里放 AI 截的配图的目录（相对仓库根）
pub const MEDIA_DIR: &str = ".pilot/media";
/// 识别提示词的版本：加了要识别的命令（比如上传图片）时 +1，已识别过的站点体检时重新识别一次
pub const DETECT_V: u32 = 1;

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
    /// 上传一张图片：含 {file}，输出里有图片的线上地址。没有就是空（引用了 .pilot/media/ 图片的文档不写入）
    #[serde(default)]
    pub upload: String,
    /// 一两句话：正文存在哪、技能怎么改、有什么要守的规矩（给你看，也写进给 AI 的说明）
    pub summary: String,
    pub detected_at: u64,
    /// 你在站点设置里确认过（没确认不生效）
    #[serde(default)]
    pub confirmed: bool,
    /// 识别出错时的原因（给你看）
    #[serde(default)]
    pub error: String,
    /// 按哪一版识别提示词识别的（见 DETECT_V）
    #[serde(default)]
    pub detect_v: u32,
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
- apply：把这份 JSON 写入线上，用 {{file}} 代表文件；\n\
- upload：上传一张图片（给文章配图用），用 {{file}} 代表图片文件，输出里会有图片的线上地址；说明里没有上传图片的命令就写空字符串。\n\
命令必须直接调用技能目录里已有的脚本（比如 python3 skills/x/scripts/x.py …），不要管道、重定向、&&、环境变量，不要编造说明里没有的参数。\n\
只输出一个 JSON 对象，不要别的文字：\n\
{{\"skill\": \"技能目录\", \"external\": true 或 false, \"get\": \"…\", \"check\": \"…\", \"apply\": \"…\", \"upload\": \"…\", \"summary\": \"一两句中文：正文存在哪、用哪个脚本怎么改、要守的规矩（比如新文档先草稿、推荐链接不进正文）\"}}\n\
正文就在仓库文件里时 external 写 false，命令都写空字符串；有几个技能时选管内容的那个。\n\n{body}"
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
    upload: String,
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
    let mut c = ContentSkill { sig, detected_at: crate::now_secs(), detect_v: DETECT_V, ..Default::default() };
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
        // 上传命令可有可无：不能用就当没有（引用了图片的文档写入时会说明原因）
        if !d.upload.trim().is_empty() && d.upload.trim() != c.apply && validate_cmd(repo, &c.skill, &d.upload, &["{file}"]).is_ok() {
            c.upload = d.upload.trim().to_string();
        }
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
    if !force && saved.as_ref().is_some_and(|s| s.sig == sig && s.error.is_empty() && s.detect_v >= DETECT_V) {
        return None;
    }
    let mut c = detect(data_dir, repo, brain, model).await;
    if let Some(s) = &saved {
        // 新识别出上传命令不算变了：它只传图片、不改文章，引用图片的文档照样要过校验
        c.confirmed = s.confirmed && s.external == c.external && s.skill == c.skill && s.get == c.get && s.check == c.check && s.apply == c.apply && (s.upload.is_empty() || s.upload == c.upload);
    }
    let _ = save(site_dir, &c);
    Some(if !c.error.is_empty() {
        format!("仓库里有内容技能 {}，{}", c.skill, c.error)
    } else if c.external && !c.confirmed {
        format!("识别到内容技能 {}：正文在仓库外（{}）。到站点设置里确认后，运营任务按它改", c.skill, c.summary)
    } else if c.external && saved.as_ref().is_some_and(|s| s.upload.is_empty()) && !c.upload.is_empty() {
        format!("内容技能 {} 能上传图片了：文章里的截图写入前先传上去", c.skill)
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

/// 文本里引用的配图：(原样写的路径，相对仓库根的文件)。认 `.pilot/media/名字`，前面可带 `/`、`./`、`../`
pub fn media_refs(text: &str) -> Vec<(String, String)> {
    let key = format!("{MEDIA_DIR}/");
    let mut out: Vec<(String, String)> = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find(&key) {
        let at = from + i;
        let name: String = text[at + key.len()..].chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')).collect();
        let start = text[..at].char_indices().rev().take_while(|(_, c)| matches!(c, '.' | '/')).last().map(|(j, _)| j).unwrap_or(at);
        let end = at + key.len() + name.len();
        from = end;
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        let r = (text[start..end].to_string(), format!("{key}{name}"));
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// 上传命令的输出里取图片的线上地址：JSON 里的 path / url / src / location，或第一个像地址的字符串
pub fn upload_url(out: &str) -> Option<String> {
    fn ok(s: &str) -> bool {
        (s.starts_with('/') && !s.starts_with("//") || s.starts_with("https://")) && !s.contains("..") && !s.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\'))
    }
    fn first(v: &serde_json::Value) -> Option<String> {
        match v {
            serde_json::Value::String(s) if ok(s) => Some(s.clone()),
            serde_json::Value::Object(m) => ["path", "url", "src", "location"].iter().find_map(|k| m.get(*k).and_then(first)).or_else(|| m.values().find_map(first)),
            serde_json::Value::Array(a) => a.iter().find_map(first),
            _ => None,
        }
    }
    let s = out.find('{')?;
    let e = out.rfind('}')?;
    first(&serde_json::from_str(out.get(s..=e)?).ok()?)
}

/// 把 JSON 里所有字符串值中的配图路径换成线上地址。`map` 按原样路径从长到短排：
/// `../.pilot/media/a.png` 要整段换掉，不能被 `.pilot/media/a.png` 先换掉后半截
fn replace_media(v: &mut serde_json::Value, map: &[(String, String)]) {
    match v {
        serde_json::Value::String(s) => {
            for (from, to) in map {
                if s.contains(from.as_str()) {
                    *s = s.replace(from.as_str(), to);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| replace_media(x, map)),
        serde_json::Value::Object(m) => m.values_mut().for_each(|x| replace_media(x, map)),
        _ => {}
    }
}

/// 试运行 / 写入前：文档里引用了 `.pilot/media/` 的配图，就用技能逐张上传，换成线上地址，
/// 另存一份到 `out_dir` 交给校验和写入（工作区里的文档不动）。没引用图片的原样返回工作区里的文件。
/// 返回 (交给技能的文件，日志)。
pub async fn with_media(cs: &ContentSkill, repo: &Path, wt: &Path, doc: &str, out_dir: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let abs = wt.join(doc);
    let text = std::fs::read_to_string(&abs).map_err(|e| format!("读不了 {doc}：{e}"))?;
    let refs = media_refs(&text);
    if refs.is_empty() {
        return Ok((abs, Vec::new()));
    }
    if cs.upload.is_empty() {
        return Err(format!(
            "{doc} 引用了 {MEDIA_DIR}/ 里的截图，但内容技能没有上传图片的命令，写进去会是坏图；到站点设置里重新识别内容技能，或把图从文档里去掉"
        ));
    }
    let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{doc} 不是有效的 JSON：{e}"))?;
    let media_root = wt.join(MEDIA_DIR).canonicalize().map_err(|_| format!("{doc} 引用了截图，但工作区里没有 {MEDIA_DIR}/"))?;
    let mut uploaded: Vec<(String, String)> = Vec::new();
    let mut map: Vec<(String, String)> = Vec::new();
    let mut log = Vec::new();
    for (written, file) in &refs {
        let url = match uploaded.iter().find(|(f, _)| f == file) {
            Some((_, u)) => u.clone(),
            None => {
                let p = wt.join(file).canonicalize().map_err(|_| format!("{doc} 引用的 {file} 不存在（截图没有提交？）"))?;
                if !p.starts_with(&media_root) || !p.is_file() {
                    return Err(format!("{doc} 引用的 {file} 不是 {MEDIA_DIR}/ 里的图片"));
                }
                let (ok, out) = run(repo, &cs.skill, &cs.upload, &["{file}"], &[("{file}", &p.to_string_lossy())]).await;
                let url = upload_url(&out).filter(|_| ok).ok_or_else(|| format!("上传 {file} 没成功：{}", out.lines().last().unwrap_or("没有输出")))?;
                log.push(format!("上传配图 {file} → {url}"));
                uploaded.push((file.clone(), url.clone()));
                url
            }
        };
        map.push((written.clone(), url));
    }
    map.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
    replace_media(&mut v, &map);
    std::fs::create_dir_all(out_dir).map_err(|e| format!("建不了临时目录：{e}"))?;
    let out = out_dir.join(doc.rsplit('/').next().unwrap_or(doc));
    let body = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::write(&out, body).map_err(|e| format!("写不了临时文件：{e}"))?;
    Ok((out, log))
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
    fn screenshots_in_documents_are_uploaded_and_replaced_before_writing() {
        let r = repo("screenshots");
        let s = "skills/blockvar-content";
        // 假的上传脚本：按文件名回一个线上地址
        std::fs::write(
            r.join("skills/blockvar-content/scripts/up.py"),
            "import sys,os,json\nif '--fail' in sys.argv: sys.exit(1)\nprint(json.dumps({'path': '/uploads/' + os.path.basename(sys.argv[1])}))\n",
        )
        .unwrap();
        let wt = r.join("wt");
        std::fs::create_dir_all(wt.join(".pilot/content")).unwrap();
        std::fs::create_dir_all(wt.join(".pilot/media")).unwrap();
        std::fs::write(wt.join(".pilot/media/a.png"), b"png").unwrap();
        std::fs::write(wt.join(".pilot/media/b.png"), b"png").unwrap();
        let doc = ".pilot/content/zh-x.json";
        let src = r#"{"title":"T","image":".pilot/media/b.png","body_html":"<p>x</p><figure><img src=\"/.pilot/media/a.png\" alt=\"a\"></figure><img src=\"../.pilot/media/a.png\">"}"#;
        std::fs::write(wt.join(doc), src).unwrap();
        assert_eq!(
            media_refs(src),
            vec![
                (".pilot/media/b.png".into(), ".pilot/media/b.png".into()),
                ("/.pilot/media/a.png".into(), ".pilot/media/a.png".into()),
                ("../.pilot/media/a.png".into(), ".pilot/media/a.png".into()),
            ]
        );
        assert!(media_refs("没有图 .pilot/media/ 和 .pilot/media/.env").is_empty());
        let t = tokio::runtime::Runtime::new().unwrap();
        let out_dir = r.join("out");
        let mut cs = ContentSkill { skill: s.into(), upload: "python3 skills/blockvar-content/scripts/up.py {file}".into(), ..Default::default() };
        let (file, log) = t.block_on(with_media(&cs, &r, &wt, doc, &out_dir)).unwrap();
        assert_eq!(file, out_dir.join("zh-x.json"), "换过地址的另存一份，工作区里的文档不动");
        assert_eq!(std::fs::read_to_string(wt.join(doc)).unwrap(), src);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["image"], "/uploads/b.png");
        assert_eq!(v["body_html"], "<p>x</p><figure><img src=\"/uploads/a.png\" alt=\"a\"></figure><img src=\"/uploads/a.png\">");
        assert_eq!(log.len(), 2, "同一张图只传一次：{log:?}");
        // 没引用图片：原样用工作区里的文件
        std::fs::write(wt.join(".pilot/content/plain.json"), r#"{"title":"T"}"#).unwrap();
        let (file, log) = t.block_on(with_media(&cs, &r, &wt, ".pilot/content/plain.json", &out_dir)).unwrap();
        assert!(file == wt.join(".pilot/content/plain.json") && log.is_empty());
        // 引用了不存在的图、跳出 .pilot/media 的、上传失败的、技能不能传图的：都不写入
        std::fs::write(wt.join(doc), r#"{"image":".pilot/media/missing.png"}"#).unwrap();
        assert!(t.block_on(with_media(&cs, &r, &wt, doc, &out_dir)).unwrap_err().contains("不存在"));
        std::fs::write(wt.join(doc), r#"{"image":".pilot/media/a.png"}"#).unwrap();
        cs.upload = "python3 skills/blockvar-content/scripts/up.py {file} --fail".into();
        assert!(t.block_on(with_media(&cs, &r, &wt, doc, &out_dir)).unwrap_err().contains("没成功"));
        cs.upload.clear();
        assert!(t.block_on(with_media(&cs, &r, &wt, doc, &out_dir)).unwrap_err().contains("没有上传图片的命令"));
        // 上传输出里取地址
        assert_eq!(upload_url("{\n  \"path\": \"/uploads/managed-ab.png\"\n}").as_deref(), Some("/uploads/managed-ab.png"));
        assert_eq!(upload_url("ok {\"id\": 3, \"url\": \"https://cdn.x/a.png\"}").as_deref(), Some("https://cdn.x/a.png"));
        assert_eq!(upload_url("{\"path\": \"//evil/a.png\"}"), None);
        assert_eq!(upload_url("uploaded"), None);
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
