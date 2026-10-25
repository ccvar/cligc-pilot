//! 邮箱验证码：AI 在手机上走注册流程时，Pilot 去你的邮箱里取这个网站刚发来的验证码 / 激活链接，直接填进手机。
//!
//! ```text
//! AI → node phone.cjs mail-code --from okx.com → 本机端口（端口和令牌在 tools/mail-bridge.json）
//!    → Pilot（本模块）用 IMAP 搜「okx.com 最近几分钟的邮件」→ 取出验证码 → adb 填进手机 → 只回「已填好」
//! ```
//!
//! - 邮箱走 IMAP + 应用专用密码（Gmail 的「应用专用密码」，QQ / 163 的「授权码」）。密码只在钥匙串里，
//!   地址和服务器在 `<数据目录>/mail.json`。
//! - **AI 读不到邮箱**：回给 AI 的只有「填好了、几位、多久前收到」，不回标题和正文 ——
//!   谁都能给你发邮件，邮件里的话不能有机会带偏 AI。
//! - 只读：EXAMINE + BODY.PEEK，不标已读、不删不移。
//! - 不取谷歌 / 苹果 / 微软和邮箱所在域发来的邮件：那是接管邮箱本身和这些账号的钥匙。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const SECRET: &str = "mail-imap";
/// 每封邮件最多取这么多字节（验证码邮件很小；带附件的大邮件只看开头）。
const FETCH_CAP: usize = 300_000;
const LOG_MAX: usize = 30;
/// 往回看多久：AI 点完「发送」再调命令，邮件可能已经先到了。
const LOOK_BACK_SECS: i64 = 180;
const POLL: Duration = Duration::from_secs(5);
const WAIT_DEFAULT: u64 = 90;
const WAIT_MAX: u64 = 180;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MailConfig {
    pub email: String,
    pub host: String,
    pub port: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LogEntry {
    pub ts: i64,
    /// 发件网站（域名）
    pub from: String,
    /// code | link
    pub kind: String,
    pub ok: bool,
    pub note: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct MailStatus {
    pub configured: bool,
    pub email: String,
    pub host: String,
    pub plus_alias: bool,
    pub log: Vec<LogEntry>,
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("mail.json")
}
fn log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("mail_log.json")
}

pub fn load_config(data_dir: &Path) -> Option<MailConfig> {
    let raw = std::fs::read(config_path(data_dir)).ok()?;
    serde_json::from_slice::<MailConfig>(&raw).ok().filter(|c| !c.email.is_empty() && !c.host.is_empty())
}

fn load_log(data_dir: &Path) -> Vec<LogEntry> {
    std::fs::read(log_path(data_dir)).ok().and_then(|r| serde_json::from_slice(&r).ok()).unwrap_or_default()
}

fn push_log(data_dir: &Path, e: LogEntry) {
    let mut log = load_log(data_dir);
    log.insert(0, e);
    log.truncate(LOG_MAX);
    if let Ok(v) = serde_json::to_vec(&log) {
        let _ = std::fs::write(log_path(data_dir), v);
    }
}

pub fn status(data_dir: &Path) -> MailStatus {
    match load_config(data_dir) {
        Some(c) => MailStatus { configured: true, plus_alias: plus_alias(&c.email), email: c.email, host: c.host, log: load_log(data_dir) },
        None => MailStatus::default(),
    }
}

fn email_domain(email: &str) -> String {
    email.rsplit_once('@').map(|(_, d)| d.trim().to_ascii_lowercase()).unwrap_or_default()
}

/// 按邮箱地址猜 IMAP 服务器。
pub fn guess_host(email: &str) -> (String, u16) {
    let d = email_domain(email);
    let host = match d.as_str() {
        "gmail.com" | "googlemail.com" => "imap.gmail.com".to_string(),
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" => "outlook.office365.com".to_string(),
        "qq.com" | "foxmail.com" => "imap.qq.com".to_string(),
        "163.com" => "imap.163.com".to_string(),
        "126.com" => "imap.126.com".to_string(),
        "yeah.net" => "imap.yeah.net".to_string(),
        "icloud.com" | "me.com" | "mac.com" => "imap.mail.me.com".to_string(),
        "yahoo.com" => "imap.mail.yahoo.com".to_string(),
        _ => format!("imap.{d}"),
    };
    (host, 993)
}

/// 支持「名字+别名@」写法的邮箱：同一个收件箱，教程用的账号可以每篇一个。
pub fn plus_alias(email: &str) -> bool {
    matches!(
        email_domain(email).as_str(),
        "gmail.com" | "googlemail.com" | "outlook.com" | "hotmail.com" | "live.com" | "icloud.com" | "me.com" | "mac.com" | "fastmail.com"
    )
}

/// 这些发件方的验证码不取：它们管着邮箱本身和手机 / 电脑的主账号。
fn blocked_sender(domain: &str, own_email: &str) -> bool {
    const BLOCKED: [&str; 12] = [
        "google.com", "gmail.com", "googlemail.com", "youtube.com", "apple.com", "icloud.com",
        "microsoft.com", "live.com", "outlook.com", "hotmail.com", "qq.com", "163.com",
    ];
    let own = email_domain(own_email);
    let hit = |b: &str| domain == b || domain.ends_with(&format!(".{b}"));
    BLOCKED.iter().any(|b| hit(b)) || (!own.is_empty() && hit(&own))
}

/// AI 给的 `--from`：域名或邮件地址 → 规范的域名。
pub fn normalize_domain(from: &str) -> Option<String> {
    let s = from.trim().to_ascii_lowercase();
    let s = s.rsplit_once('@').map(|(_, d)| d.to_string()).unwrap_or(s);
    let s = s.trim_start_matches("https://").trim_start_matches("http://");
    let s = s.split(['/', '?', '#', ':']).next().unwrap_or("");
    let s = s.strip_prefix("www.").unwrap_or(s).trim_matches('.');
    let ok = s.contains('.') && s.len() <= 253 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    ok.then(|| s.to_string())
}

fn sender_matches(addr: &str, domain: &str) -> bool {
    let d = email_domain(addr);
    d == domain || d.ends_with(&format!(".{domain}"))
}

// ---------------------------------------------------------------------------------------------
// 从邮件里认验证码 / 激活链接

const CODE_WORDS: [&str; 18] = [
    "验证码", "校验码", "驗證碼", "動態碼", "动态码", "确认码", "安全码", "激活码", "認証コード", "인증",
    "verification", "verify", "code", "otp", "passcode", "pin", "one-time", "security",
];

fn has_code_word(s: &str) -> bool {
    let l = s.to_lowercase();
    CODE_WORDS.iter().any(|w| l.contains(w))
}

/// 一串文字里的候选验证码：4–8 位数字，或 6–8 位大写字母加数字。分开写的「1 2 3 4 5 6」「123 456」会拼起来。
fn code_candidates(cs: &[char]) -> Vec<(usize, usize, String)> {
    let alnum = |c: char| c.is_ascii_alphanumeric();
    let mut raw: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if alnum(cs[i]) {
            let a = i;
            while i < cs.len() && alnum(cs[i]) {
                i += 1;
            }
            raw.push((a, i));
        } else {
            i += 1;
        }
    }
    let text = |a: usize, b: usize| cs[a..b].iter().collect::<String>();
    let digits = |a: usize, b: usize| cs[a..b].iter().all(|c| c.is_ascii_digit());
    let mut out = Vec::new();
    let mut k = 0;
    while k < raw.len() {
        let (a, mut b) = raw[k];
        let mut t = text(a, b);
        let mut next = k + 1;
        // 同样长度（1 位或 3 位）的数字组，中间隔一个空格或横线 → 拼成一个
        if digits(a, b) && (b - a == 1 || b - a == 3) {
            let w = b - a;
            let mut j = k + 1;
            let mut joined = t.clone();
            let mut end = b;
            while j < raw.len() {
                let (na, nb) = raw[j];
                if na != end + 1 || !(cs[end] == ' ' || cs[end] == '-') || nb - na != w || !digits(na, nb) || joined.len() + w > 8 {
                    break;
                }
                joined.push_str(&text(na, nb));
                end = nb;
                j += 1;
            }
            if j > k + 1 && joined.len() >= 4 {
                t = joined;
                b = end;
                next = j;
            }
        }
        k = next;
        let n = t.len();
        if !(4..=8).contains(&n) {
            continue;
        }
        let d = t.chars().filter(|c| c.is_ascii_digit()).count();
        let letters = n - d;
        let ok = if letters == 0 { true } else { n >= 6 && d >= 2 && t.chars().all(|c| c.is_ascii_digit() || c.is_ascii_uppercase()) };
        if !ok {
            continue;
        }
        // 网址、邮箱、电话、金额、时间里的数字不算
        let prev = a.checked_sub(1).map(|p| cs[p]);
        let after = cs.get(b).copied();
        let after2 = cs.get(b + 1).copied();
        if matches!(prev, Some('.' | '/' | '@' | '=' | '&' | '#' | '%' | '+' | '_' | '$' | '¥' | '€' | '£')) {
            continue;
        }
        if matches!(after, Some('@' | '/' | '=' | '&' | '%' | '_')) || (matches!(after, Some('.' | ':' | ',')) && after2.is_some_and(|c| c.is_ascii_digit())) {
            continue;
        }
        if prev == Some('-') && a >= 2 && cs[a - 2].is_ascii_digit() {
            continue;
        }
        out.push((a, b, t));
    }
    out
}

/// 从标题和正文里认出验证码：要挨着「验证码 / code」这类词，6 位优先。
pub fn extract_code(subject: &str, body: &str) -> Option<String> {
    let mut best: Option<(i32, String)> = None;
    for (in_subject, s) in [(true, subject), (false, body)] {
        let cs: Vec<char> = s.chars().collect();
        let subject_word = in_subject && has_code_word(s);
        for (a, b, t) in code_candidates(&cs) {
            let before: String = cs[a.saturating_sub(40)..a].iter().collect();
            let after: String = cs[b..(b + 24).min(cs.len())].iter().collect();
            let near_before = has_code_word(&before);
            let near_after = has_code_word(&after);
            if !(near_before || near_after || subject_word) {
                continue;
            }
            let mut score = if t.len() == 6 { 3 } else { 1 };
            if t.chars().all(|c| c.is_ascii_digit()) {
                score += 1;
            }
            if near_before {
                score += 5;
            } else if near_after {
                score += 3;
            }
            if in_subject {
                score += 2;
            }
            if t.len() == 4 && t.parse::<u32>().is_ok_and(|y| (1990..=2099).contains(&y)) {
                score -= 4;
            }
            if best.as_ref().is_none_or(|(s0, _)| score > *s0) {
                best = Some((score, t));
            }
        }
    }
    best.filter(|(s, _)| *s >= 3).map(|(_, t)| t)
}

fn url_host(u: &str) -> Option<String> {
    let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    let host = host.split(':').next()?;
    Some(host.to_ascii_lowercase())
}

/// 邮件里这个网站自己域名下的激活 / 验证链接（不打开退订、隐私、帮助这类）。
pub fn extract_link(html: &str, text: &str, domain: &str) -> Option<String> {
    let mut cands: Vec<String> = Vec::new();
    for q in ['"', '\''] {
        let pat = format!("href={q}");
        let mut rest = html;
        while let Some(p) = rest.find(&pat) {
            rest = &rest[p + pat.len()..];
            if let Some(e) = rest.find(q) {
                cands.push(rest[..e].replace("&amp;", "&").trim().to_string());
                rest = &rest[e..];
            }
        }
    }
    for w in text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '(' | ')' | '[' | ']')) {
        if w.starts_with("https://") || w.starts_with("http://") {
            cands.push(w.trim_end_matches(['.', ',', '。', '，']).to_string());
        }
    }
    const GOOD: [&str; 11] = ["verify", "verif", "confirm", "activat", "validat", "register", "signup", "sign-up", "token", "验证", "激活"];
    const BAD: [&str; 8] = ["unsubscribe", "privacy", "terms", "help", "support", "preferences", "download", "app-store"];
    let mut best: Option<(i32, String)> = None;
    for u in cands {
        let Some(host) = url_host(&u) else { continue };
        if !(host == domain || host.ends_with(&format!(".{domain}"))) {
            continue;
        }
        if u.len() > 2000 || u.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\'' | '"' | '`' | '<' | '>' | '\\')) {
            continue;
        }
        let l = u.to_lowercase();
        if BAD.iter().any(|b| l.contains(b)) {
            continue;
        }
        let score = GOOD.iter().filter(|g| l.contains(*g)).count() as i32;
        if score > 0 && best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, u));
        }
    }
    best.map(|(_, u)| u)
}

// ---------------------------------------------------------------------------------------------
// 最小 IMAP 客户端：LOGIN / LIST / EXAMINE / UID SEARCH / UID FETCH / LOGOUT，只读。

type Tls = tokio_rustls::client::TlsStream<tokio::net::TcpStream>;

struct Imap {
    r: BufReader<Tls>,
    n: u32,
}

/// 一条未打标签的服务器回应：文字部分 + 其中的字面量（邮件原文）。
struct Resp {
    text: String,
    lits: Vec<Vec<u8>>,
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn literal_len(line: &str) -> Option<usize> {
    let s = line.strip_suffix('}')?;
    let p = s.rfind('{')?;
    s[p + 1..].trim_end_matches('+').parse().ok()
}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, String> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_platform_verifier()
        .map_err(|e| e.to_string())?
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

fn login_error(e: String) -> String {
    let l = e.to_lowercase();
    if l.contains("application-specific password") || l.contains("app password") {
        "要用「应用专用密码」，不能用邮箱的登录密码：Gmail 在 Google 账号里打开两步验证后生成一个。".into()
    } else if l.contains("authenticationfailed") || l.contains("invalid credentials") || l.contains("login fail") || l.contains("authentication failed") || l.contains("auth") {
        "邮箱地址或应用专用密码不对。".into()
    } else {
        format!("邮箱登录失败：{e}")
    }
}

impl Imap {
    async fn connect(host: &str, port: u16) -> Result<Imap, String> {
        let t = Duration::from_secs(15);
        let tcp = tokio::time::timeout(t, tokio::net::TcpStream::connect((host, port)))
            .await
            .map_err(|_| format!("连不上邮箱服务器 {host}（15 秒超时）"))?
            .map_err(|e| format!("连不上邮箱服务器 {host}：{e}"))?;
        let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| format!("服务器地址不对：{e}"))?;
        let tls = tokio::time::timeout(t, tokio_rustls::TlsConnector::from(tls_config()?).connect(name, tcp))
            .await
            .map_err(|_| "和邮箱服务器建立加密连接超时".to_string())?
            .map_err(|e| format!("和邮箱服务器建立加密连接失败：{e}"))?;
        let mut me = Imap { r: BufReader::new(tls), n: 0 };
        let greet = me.line().await?;
        if !greet.starts_with("* OK") && !greet.starts_with("* PREAUTH") {
            return Err(format!("邮箱服务器拒绝连接：{}", greet.chars().take(200).collect::<String>()));
        }
        Ok(me)
    }

    async fn line(&mut self) -> Result<String, String> {
        let mut buf = Vec::new();
        tokio::time::timeout(Duration::from_secs(30), self.r.read_until(b'\n', &mut buf))
            .await
            .map_err(|_| "邮箱服务器没有响应".to_string())?
            .map_err(|e| format!("读取邮箱失败：{e}"))?;
        if buf.is_empty() {
            return Err("邮箱服务器断开了连接".into());
        }
        Ok(String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string())
    }

    async fn run(&mut self, cmd: &str) -> Result<Vec<Resp>, String> {
        self.n += 1;
        let tag = format!("p{}", self.n);
        let w = self.r.get_mut();
        w.write_all(format!("{tag} {cmd}\r\n").as_bytes()).await.map_err(|e| format!("写入邮箱失败：{e}"))?;
        w.flush().await.map_err(|e| format!("写入邮箱失败：{e}"))?;
        let done = format!("{tag} ");
        let mut out = Vec::new();
        loop {
            let mut l = self.line().await?;
            if let Some(rest) = l.strip_prefix(&done) {
                return if rest.starts_with("OK") { Ok(out) } else { Err(rest.chars().take(300).collect()) };
            }
            let mut resp = Resp { text: String::new(), lits: Vec::new() };
            while let Some(n) = literal_len(&l) {
                resp.text.push_str(&l);
                if n > FETCH_CAP + 4096 {
                    return Err("邮件太大".into());
                }
                let mut b = vec![0u8; n];
                tokio::time::timeout(Duration::from_secs(30), self.r.read_exact(&mut b))
                    .await
                    .map_err(|_| "读取邮件超时".to_string())?
                    .map_err(|e| format!("读取邮件失败：{e}"))?;
                resp.lits.push(b);
                // 字面量之后，这一行剩下的部分
                l = self.line().await?;
            }
            resp.text.push_str(&l);
            out.push(resp);
        }
    }

    async fn login(host: &str, port: u16, email: &str, pass: &str) -> Result<Imap, String> {
        let mut im = Imap::connect(host, port).await?;
        im.run(&format!("LOGIN {} {}", quote(email), quote(pass))).await.map_err(login_error)?;
        Ok(im)
    }

    /// 垃圾邮件文件夹（LIST 里带 \Junk 的那个；Gmail 中文界面叫别的名字，所以不能写死）。
    async fn junk(&mut self) -> Option<String> {
        let list = self.run("LIST \"\" \"*\"").await.ok()?;
        let r = list.into_iter().find(|r| r.text.to_lowercase().contains("\\junk"))?;
        if let Some(l) = r.lits.first() {
            return Some(String::from_utf8_lossy(l).into_owned());
        }
        let t = r.text.trim_end();
        if let Some(s) = t.strip_suffix('"') {
            return s.rfind('"').map(|p| s[p + 1..].to_string());
        }
        t.rsplit(' ').next().map(|s| s.to_string())
    }

    /// 某个文件夹里，最近一天 domain 发来的邮件（最新几封）：(文件夹|UID, 收到时间, 原文)。
    async fn recent(&mut self, mailbox: &str, domain: &str, since_ts: i64) -> Result<Vec<(String, i64, Vec<u8>)>, String> {
        self.run(&format!("EXAMINE {}", quote(mailbox))).await?;
        let day = chrono::DateTime::from_timestamp(since_ts - 86_400, 0).unwrap_or_default().format("%d-%b-%Y").to_string();
        let found = self.run(&format!("UID SEARCH SINCE {day} FROM {}", quote(domain))).await?;
        let mut uids: Vec<u32> = found
            .iter()
            .filter_map(|r| r.text.strip_prefix("* SEARCH"))
            .flat_map(|s| s.split_whitespace().filter_map(|x| x.parse().ok()).collect::<Vec<u32>>())
            .collect();
        uids.sort_unstable();
        let last: Vec<String> = uids.iter().rev().take(5).map(|u| u.to_string()).collect();
        if last.is_empty() {
            return Ok(Vec::new());
        }
        let rows = self.run(&format!("UID FETCH {} (UID INTERNALDATE BODY.PEEK[]<0.{FETCH_CAP}>)", last.join(","))).await?;
        let mut out = Vec::new();
        for r in rows {
            let Some(body) = r.lits.last() else { continue };
            let uid = r.text.split("UID ").nth(1).and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next()).unwrap_or("");
            let date = r.text.split("INTERNALDATE \"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("");
            let ts = parse_internaldate(date).unwrap_or(0);
            if uid.is_empty() || ts < since_ts {
                continue;
            }
            out.push((format!("{mailbox}|{uid}"), ts, body.clone()));
        }
        Ok(out)
    }

    async fn logout(mut self) {
        let _ = self.run("LOGOUT").await;
    }
}

/// IMAP 的 INTERNALDATE：`25-Oct-2026 10:00:00 +0000`（日期一位数时前面是空格）。
fn parse_internaldate(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_str(s.trim(), "%d-%b-%Y %H:%M:%S %z").ok().map(|d| d.timestamp())
}

/// 接邮箱：先真登录一次确认能用，再存（地址进 mail.json，密码进钥匙串）。
pub async fn connect(data_dir: &Path, email: &str, password: &str) -> Result<MailStatus, String> {
    let email = email.trim().to_string();
    if !email.contains('@') || email_domain(&email).is_empty() {
        return Err("邮箱地址不对".into());
    }
    // 应用专用密码显示时带空格（abcd efgh ijkl mnop），实际不含
    let pass: String = password.chars().filter(|c| !c.is_whitespace()).collect();
    if pass.is_empty() {
        return Err("请填应用专用密码".into());
    }
    let (host, port) = guess_host(&email);
    let mut im = Imap::login(&host, port, &email, &pass).await?;
    im.run("EXAMINE \"INBOX\"").await.map_err(|e| format!("打不开收件箱：{e}"))?;
    im.logout().await;
    crate::keychain::set_named_secret(SECRET, &pass)?;
    let cfg = MailConfig { email, host, port };
    std::fs::write(config_path(data_dir), serde_json::to_vec_pretty(&cfg).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(status(data_dir))
}

pub fn remove(data_dir: &Path) -> Result<(), String> {
    let _ = crate::keychain::delete_named_secret(SECRET);
    let _ = std::fs::remove_file(config_path(data_dir));
    let _ = std::fs::remove_file(log_path(data_dir));
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// 本机端口：phone.cjs 的 mail-* 命令从这里取

#[derive(Deserialize)]
struct Req {
    token: String,
    op: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    serial: String,
    #[serde(default)]
    wait: u64,
    /// 再填一次上次那个验证码（上次没填进去时）
    #[serde(default)]
    reuse: bool,
}

/// 已经填过的邮件，下次取新的（重新发送验证码时不会把旧的再填一遍）。
static USED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn err(code: &str, detail: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "ok": false, "code": code, "detail": detail.into() })
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn bridge_path(data_dir: &Path) -> PathBuf {
    data_dir.join("tools").join("mail-bridge.json")
}

/// 起本机端口（只听 127.0.0.1），把端口和令牌写进 tools/mail-bridge.json 给 phone.cjs 读。
pub async fn serve(data_dir: PathBuf) {
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[mail] 本机端口绑不上，邮箱验证码不可用: {e}");
            return;
        }
    };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else { return };
    let token = uuid::Uuid::new_v4().simple().to_string();
    let p = bridge_path(&data_dir);
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(&data_dir));
    let _ = std::fs::write(&p, serde_json::json!({ "port": port, "token": token }).to_string());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    let token = Arc::new(token);
    let data_dir = Arc::new(data_dir);
    // 一次只办一件：两条命令同时取同一个网站的邮件会抢同一封
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let (token, data_dir, lock) = (token.clone(), data_dir.clone(), lock.clone());
        tokio::spawn(async move {
            let (rd, mut wr) = stream.into_split();
            let mut r = BufReader::new(rd.take(16_384));
            let mut line = String::new();
            let reply = match tokio::time::timeout(Duration::from_secs(10), r.read_line(&mut line)).await {
                Ok(Ok(_)) => match serde_json::from_str::<Req>(&line) {
                    Ok(req) if req.token == *token => {
                        let _g = lock.lock().await;
                        dispatch(&data_dir, req).await
                    }
                    Ok(_) => err("bad_token", "令牌不对"),
                    Err(_) => err("bad_request", "请求格式不对"),
                },
                _ => err("bad_request", "没收到请求"),
            };
            let _ = wr.write_all(format!("{reply}\n").as_bytes()).await;
        });
    }
}

async fn dispatch(data_dir: &Path, req: Req) -> serde_json::Value {
    let cfg = load_config(data_dir);
    if req.op == "info" {
        return match cfg {
            Some(c) => {
                let plus = plus_alias(&c.email);
                let alias = c.email.split_once('@').map(|(u, d)| format!("{u}+tutorial1@{d}")).unwrap_or_default();
                serde_json::json!({ "ok": true, "configured": true, "email": c.email, "plus_alias": plus, "alias_example": if plus { alias } else { String::new() } })
            }
            None => serde_json::json!({ "ok": true, "configured": false }),
        };
    }
    let kind = match req.op.as_str() {
        "code" => "code",
        "link" => "link",
        _ => return err("bad_request", "不认识的操作"),
    };
    let Some(cfg) = cfg else {
        return err("not_configured", "Pilot 还没接邮箱：这一步交给用户，或者请用户在「设置 → 安卓手机 → 邮箱验证码」里接上。");
    };
    let Some(domain) = normalize_domain(&req.from) else {
        return err("bad_args", "--from 要写发邮件的网站域名，比如 okx.com");
    };
    if blocked_sender(&domain, &cfg.email) {
        return err("blocked", format!("{domain} 的邮件不代取：谷歌、苹果、微软和邮箱服务商自己的验证码管着你的主账号，请用户自己处理。"));
    }
    if !req.serial.is_empty() && !req.serial.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_')) {
        return err("bad_args", "--serial 不对");
    }
    let pass = match crate::keychain::get_named_secret(SECRET) {
        Ok(p) => p,
        Err(_) => return err("not_configured", "钥匙串里没有邮箱的应用专用密码：请用户在设置里重新接一下邮箱。"),
    };
    let wait = if req.wait == 0 { WAIT_DEFAULT } else { req.wait.clamp(10, WAIT_MAX) };
    let res = fetch_and_fill(data_dir, &cfg, &pass, &domain, kind, &req.serial, wait, req.reuse).await;
    let (ok, note) = match &res {
        Ok(v) => (true, v.get("note").and_then(|n| n.as_str()).unwrap_or("").to_string()),
        Err(e) => (false, e.get("detail").and_then(|n| n.as_str()).unwrap_or("").chars().take(120).collect()),
    };
    push_log(data_dir, LogEntry { ts: now(), from: domain.clone(), kind: kind.into(), ok, note });
    match res {
        Ok(mut v) => {
            if let Some(o) = v.as_object_mut() {
                o.remove("note");
            }
            v
        }
        Err(e) => e,
    }
}

#[allow(clippy::too_many_arguments)]
async fn fetch_and_fill(
    data_dir: &Path,
    cfg: &MailConfig,
    pass: &str,
    domain: &str,
    kind: &str,
    serial: &str,
    wait: u64,
    reuse: bool,
) -> Result<serde_json::Value, serde_json::Value> {
    let since = now() - LOOK_BACK_SECS;
    let deadline = std::time::Instant::now() + Duration::from_secs(wait);
    let mut im = Imap::login(&cfg.host, cfg.port, &cfg.email, pass).await.map_err(|e| err("mail_login", e))?;
    let mut boxes = vec!["INBOX".to_string()];
    if let Some(j) = im.junk().await {
        boxes.push(j);
    }
    let parser = mail_parser::MessageParser::default();
    let mut saw_mail = false;
    let found = 'poll: loop {
        let mut msgs = Vec::new();
        for mb in &boxes {
            match im.recent(mb, domain, since).await {
                Ok(m) => msgs.extend(m),
                Err(e) if mb == "INBOX" => {
                    im.logout().await;
                    return Err(err("mail_failed", format!("读收件箱失败：{e}")));
                }
                Err(_) => {}
            }
        }
        msgs.sort_by_key(|m| std::cmp::Reverse(m.1));
        for (key, ts, raw) in msgs {
            let key = format!("{}|{key}", cfg.email);
            let used = USED.lock().map(|u| u.contains(&key)).unwrap_or(false);
            if used && !reuse {
                continue;
            }
            let Some(msg) = parser.parse(&raw) else { continue };
            let from = msg.from().and_then(|a| a.first()).and_then(|a| a.address()).unwrap_or("");
            if !sender_matches(from, domain) {
                continue;
            }
            saw_mail = true;
            let subject = msg.subject().unwrap_or("");
            let text = msg.body_text(0).map(|t| t.into_owned()).unwrap_or_default();
            let hit = if kind == "code" {
                extract_code(subject, &text)
            } else {
                let html = msg.body_html(0).map(|t| t.into_owned()).unwrap_or_default();
                extract_link(&html, &text, domain)
            };
            if let Some(v) = hit {
                break 'poll (key, ts, v);
            }
        }
        if std::time::Instant::now() + POLL > deadline {
            im.logout().await;
            return Err(if saw_mail && kind == "code" {
                err("no_code_in_mail", format!("收到了 {domain} 的邮件，但没认出验证码：可能是激活链接（改用 mail-link），否则请用户看一下邮件。"))
            } else if saw_mail {
                err("no_link_in_mail", format!("收到了 {domain} 的邮件，但里面没有 {domain} 的验证 / 激活链接：请用户看一下邮件。"))
            } else {
                err("no_mail", format!("{wait} 秒内没收到 {domain} 的邮件（也查了垃圾邮件）：确认已经点了「发送」、邮箱填的是 {}，可以再点一次「重新发送」后重试。", cfg.email))
            });
        }
        tokio::time::sleep(POLL).await;
    };
    im.logout().await;
    let (key, ts, value) = found;
    let age = (now() - ts).max(0);
    if kind == "code" {
        adb(data_dir, serial, &["shell", "input", "text", &value]).await.map_err(|e| err("fill_failed", e))?;
        if let Ok(mut u) = USED.lock() {
            u.insert(key);
        }
        Ok(serde_json::json!({ "ok": true, "filled": true, "digits": value.len(), "from": domain, "received_secs_ago": age, "note": "验证码已填到手机" }))
    } else {
        let host = url_host(&value).unwrap_or_default();
        let out = adb(data_dir, serial, &["shell", "am", "start", "-a", "android.intent.action.VIEW", "-d", &format!("'{value}'")])
            .await
            .map_err(|e| err("open_failed", e))?;
        if out.contains("Error") {
            return Err(err("open_failed", "手机打不开这个链接"));
        }
        if let Ok(mut u) = USED.lock() {
            u.insert(key);
        }
        Ok(serde_json::json!({ "ok": true, "opened": true, "host": host, "from": domain, "received_secs_ago": age, "note": format!("在手机上打开了 {host} 的链接") }))
    }
}

async fn adb(data_dir: &Path, serial: &str, args: &[&str]) -> Result<String, String> {
    let adb = crate::phone::adb_path(data_dir).ok_or("没有找到 adb")?;
    let mut c = tokio::process::Command::new(adb);
    if !serial.is_empty() {
        c.args(["-s", serial]);
    }
    c.args(args).kill_on_drop(true);
    let o = tokio::time::timeout(Duration::from_secs(20), c.output())
        .await
        .map_err(|_| "adb 没有响应".to_string())?
        .map_err(|e| format!("运行 adb 失败：{e}"))?;
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    if !o.status.success() {
        let e = String::from_utf8_lossy(&o.stderr).trim().chars().take(200).collect::<String>();
        return Err(if e.is_empty() { format!("adb 退出码 {:?}", o.status.code()) } else { e });
    }
    Ok(stdout + &String::from_utf8_lossy(&o.stderr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(extract_code("", "您的验证码是：482913，10 分钟内有效。").as_deref(), Some("482913"));
        assert_eq!(extract_code("Your OKX verification code", "Hi,\n\n  739201\n\nThe code expires in 10 minutes. © 2026 OKX").as_deref(), Some("739201"));
        assert_eq!(extract_code("", "Use 5521 as your one-time passcode.").as_deref(), Some("5521"));
        assert_eq!(extract_code("", "Verification code: 1 2 3 4 5 6").as_deref(), Some("123456"));
        assert_eq!(extract_code("", "Your code is 123 456.").as_deref(), Some("123456"));
        assert_eq!(extract_code("", "Your code: K7P2Q9").as_deref(), Some("K7P2Q9"));
        // 没有「验证码 / code」这类词 → 不乱认
        assert_eq!(extract_code("Welcome", "Order 482913 shipped. Call 400-820-8820."), None);
        // 网址、金额里的数字不认
        assert_eq!(extract_code("", "verify at https://x.com/v?id=773311 code below").as_deref(), None);
        assert_eq!(extract_code("", "验证码 ¥123456").as_deref(), None);
    }

    #[test]
    fn links() {
        let html = r#"<a href="https://www.okx.com/account/verify?token=abc&amp;x=1">Verify</a> <a href="https://okx.com/unsubscribe">u</a> <a href="https://evil.com/verify">x</a>"#;
        assert_eq!(extract_link(html, "", "okx.com").as_deref(), Some("https://www.okx.com/account/verify?token=abc&x=1"));
        assert_eq!(extract_link(r#"<a href="https://okx.com.evil.io/verify">x</a>"#, "", "okx.com"), None);
        assert_eq!(extract_link("", "Confirm: https://app.example.com/confirm/abc.", "example.com").as_deref(), Some("https://app.example.com/confirm/abc"));
    }

    #[test]
    fn domains() {
        assert_eq!(normalize_domain("noreply@mail.okx.com").as_deref(), Some("mail.okx.com"));
        assert_eq!(normalize_domain("https://www.okx.com/join").as_deref(), Some("okx.com"));
        assert_eq!(normalize_domain("okx"), None);
        assert!(sender_matches("no-reply@mail.okx.com", "okx.com"));
        assert!(!sender_matches("x@notokx.com", "okx.com"));
        assert!(blocked_sender("accounts.google.com", "a@gmail.com"));
        assert!(blocked_sender("corp.example", "me@corp.example"));
        assert!(!blocked_sender("okx.com", "a@gmail.com"));
    }

    #[test]
    fn imap_bits() {
        assert_eq!(literal_len("* 1 FETCH (UID 5 BODY[]<0> {1234}"), Some(1234));
        assert_eq!(literal_len("* OK done"), None);
        assert_eq!(quote(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(parse_internaldate(" 5-Oct-2026 10:00:00 +0000"), Some(1791194400));
        assert_eq!(guess_host("x@gmail.com").0, "imap.gmail.com");
        assert!(plus_alias("x@gmail.com") && !plus_alias("x@qq.com"));
    }

    /// 真连一次 Gmail（假密码）：TLS、问候、LOGIN 失败的提示都对。`cargo test -- --ignored live_gmail`
    #[tokio::test]
    #[ignore]
    async fn live_gmail_rejects_bad_password() {
        let e = Imap::login("imap.gmail.com", 993, "pilot-test-nonexistent@gmail.com", "abcdabcdabcdabcd").await.err().unwrap();
        assert!(e.contains("不对") || e.contains("应用专用密码"), "{e}");
    }

    /// 本机端口：令牌对才办事；没接邮箱时 info 说 configured=false、code 说 not_configured。
    #[tokio::test]
    async fn bridge_info_and_token() {
        let d = std::env::temp_dir().join(format!("mail-t-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        tokio::spawn(serve(d.clone()));
        let p = bridge_path(&d);
        for _ in 0..50 {
            if p.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let b: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        let port = b["port"].as_u64().unwrap() as u16;
        async fn ask(port: u16, body: serde_json::Value) -> serde_json::Value {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(format!("{body}\n").as_bytes()).await.unwrap();
            let mut out = String::new();
            BufReader::new(s).read_line(&mut out).await.unwrap();
            serde_json::from_str::<serde_json::Value>(&out).unwrap()
        }
        let tok = b["token"].as_str().unwrap().to_string();
        assert_eq!(ask(port, serde_json::json!({"token": "x", "op": "info"})).await["code"], "bad_token");
        assert_eq!(ask(port, serde_json::json!({"token": tok, "op": "info"})).await["configured"], false);
        assert_eq!(ask(port, serde_json::json!({"token": tok, "op": "code", "from": "okx.com"})).await["code"], "not_configured");
        // 浏览器网页往本机端口发的 HTTP 请求不是 JSON → 直接拒
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(b"POST / HTTP/1.1\r\n\r\n").await.unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).await.unwrap();
        assert!(out.contains("bad_request"));
        std::fs::remove_dir_all(&d).ok();
    }
}
