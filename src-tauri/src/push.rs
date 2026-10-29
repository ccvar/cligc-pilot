//! 手机通知与远程确认（Telegram 机器人）：人不在电脑前，托管日报、站点打不开、推广链接失效、自动上线 / 回滚这些
//! 系统通知同时发一份到你的 Telegram；AI 改好、等你确认上线的改动带「上线 / 驳回」按钮，在手机上点一下就行。
//!
//! - 机器人是你自己在 @BotFather 建的，Token 只进钥匙串；不需要服务器：Pilot 开着时用长轮询（getUpdates）收你的消息和按钮；
//! - 只认绑定的那一个私聊：绑定时 Pilot 给一个 10 分钟内有效的配对码，你用它给机器人发 `/start <配对码>`；其它人发什么都不理；
//! - 按钮只对应发出去时记下的那条改动（短键 → 站点 + 条目），点的时候再查一次状态，已经处理过的不会重复上线。
//!
//! 设置存在数据目录的 telegram.json。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const SECRET: &str = "telegram-bot-token";
const PAIR_SECS: u64 = 600;
/// 发出去的按钮记多久（之后再点就提示「过期了，到电脑上处理」）
const PENDING_SECS: u64 = 30 * 86_400;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Settings {
    /// 发通知（绑定后默认打开）
    #[serde(default)]
    pub enabled: bool,
    /// 允许在手机上点「上线」
    #[serde(default = "yes")]
    pub allow_approve: bool,
    #[serde(default)]
    pub bot_name: String,
    #[serde(default)]
    pub chat_id: i64,
    #[serde(default)]
    pub chat_name: String,
    #[serde(default)]
    pub pair_code: String,
    #[serde(default)]
    pub pair_until: u64,
    /// getUpdates 的下一个 offset
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub last_err: String,
    /// 按钮短键 → (站点 id, 条目 id, 发出时间)
    #[serde(default)]
    pub pending: BTreeMap<String, (String, String, u64)>,
}

fn yes() -> bool {
    true
}

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("telegram.json")
}

static LOCK: Mutex<()> = Mutex::new(());

pub fn load(data_dir: &Path) -> Settings {
    crate::store_file::read_or_quarantine(&path(data_dir))
}

fn save(data_dir: &Path, s: &Settings) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let tmp = path(data_dir).with_extension("json.tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, path(data_dir));
        }
    }
}

/// 读-改-写（轮询和界面可能同时改）
fn update<R>(data_dir: &Path, f: impl FnOnce(&mut Settings) -> R) -> R {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load(data_dir);
    let r = f(&mut s);
    save(data_dir, &s);
    r
}

fn token() -> Option<String> {
    crate::keychain::get_named_secret(SECRET).ok().filter(|t| !t.trim().is_empty())
}

fn now() -> u64 {
    crate::now_secs()
}

/// Token 的样子：数字:字母数字（BotFather 给的）
pub fn valid_token(t: &str) -> bool {
    let Some((id, rest)) = t.trim().split_once(':') else { return false };
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) && rest.len() >= 30 && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

async fn call(token: &str, method: &str, body: Value, timeout: u64) -> Result<Value, String> {
    let resp = crate::cf::http()
        .post(format!("https://api.telegram.org/bot{token}/{method}"))
        .json(&body)
        .timeout(Duration::from_secs(timeout))
        .send()
        .await
        .map_err(|e| if e.is_timeout() { "连 Telegram 超时（网络连不上 api.telegram.org？）".to_string() } else { "连不上 Telegram（网络连不上 api.telegram.org？）".to_string() })?;
    let v: Value = resp.json().await.map_err(|_| "Telegram 的响应无法解析".to_string())?;
    if v["ok"].as_bool() != Some(true) {
        let d = v["description"].as_str().unwrap_or("未知错误");
        return Err(match v["error_code"].as_i64() {
            Some(401) | Some(404) => "Telegram 不认这个 Token（在 @BotFather 里复制完整的 Token）".into(),
            Some(409) => "这个机器人的消息被别的程序在收（同一个 Token 不能同时给两处用长轮询）".into(),
            Some(403) => "机器人被你屏蔽了，或者你还没给它发过消息".into(),
            _ => format!("Telegram：{d}"),
        });
    }
    Ok(v["result"].clone())
}

async fn send_to(token: &str, chat: i64, text: &str, buttons: Option<Value>) -> Result<Value, String> {
    let mut body = json!({ "chat_id": chat, "text": clip(text, 3900), "disable_web_page_preview": true });
    if let Some(b) = buttons {
        body["reply_markup"] = json!({ "inline_keyboard": b });
    }
    call(token, "sendMessage", body, 20).await
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

// ───────────────────────── 往外发 ─────────────────────────

/// 系统通知 + （打开了的话）发一份到 Telegram。各处的系统通知都走这里。
pub fn notify(app: &tauri::AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
    send(app, format!("{title}\n{body}"));
}

/// 只发 Telegram（不弹系统通知）
pub fn send(app: &tauri::AppHandle, text: String) {
    use tauri::Manager;
    let data_dir = app.state::<crate::AppState>().data_dir.clone();
    let s = load(&data_dir);
    if !s.enabled || s.chat_id == 0 {
        return;
    }
    let Some(t) = token() else { return };
    tauri::async_runtime::spawn(async move {
        if let Err(e) = send_to(&t, s.chat_id, &text, None).await {
            update(&data_dir, |s| s.last_err = e);
        }
    });
}

/// AI 改好、检查通过、等你确认上线的改动：发一条带「上线 / 驳回」按钮的消息。
pub fn proposal(app: &tauri::AppHandle, site: &crate::sites::Site, item_id: &str) {
    use tauri::Manager;
    let st = app.state::<crate::AppState>();
    let data_dir = st.data_dir.clone();
    let s = load(&data_dir);
    if !s.enabled || s.chat_id == 0 {
        return;
    }
    let Some(t) = token() else { return };
    let Some(item) = crate::site_ops::load_queue(&st.sites.data_dir(&site.id)).into_iter().find(|i| i.id == item_id) else { return };
    let text = proposal_text(&site.name, &item);
    let buttons = if s.allow_approve && item.status == "proposed" && !item.branch.is_empty() { Some(buttons_for(&data_dir, &site.id, &item.id)) } else { None };
    tauri::async_runtime::spawn(async move {
        if let Err(e) = send_to(&t, s.chat_id, &text, buttons).await {
            update(&data_dir, |s| s.last_err = e);
        }
    });
}

fn proposal_text(site_name: &str, i: &crate::site_ops::QueueItem) -> String {
    let mut t = format!("【{site_name}】等你确认上线\n{}\n", i.opp.title);
    if !i.ai_summary.is_empty() {
        t.push_str(&format!("\n改了什么：{}\n", clip(&i.ai_summary, 600)));
    }
    if !i.files.is_empty() {
        let more = if i.files.len() > 5 { format!(" 等 {} 个文件", i.files.len()) } else { String::new() };
        t.push_str(&format!("文件：{}{more}\n", i.files.iter().take(5).cloned().collect::<Vec<_>>().join("、")));
    }
    if !i.verdict.is_empty() {
        t.push_str(&format!("检查：{}\n", clip(&i.verdict, 300)));
    }
    if !i.pr_url.is_empty() {
        t.push_str(&format!("PR：{}\n", i.pr_url));
    }
    t
}

fn buttons_for(data_dir: &Path, site_id: &str, item_id: &str) -> Value {
    let key = uuid::Uuid::new_v4().simple().to_string()[..10].to_string();
    update(data_dir, |s| {
        let cut = now().saturating_sub(PENDING_SECS);
        s.pending.retain(|_, v| v.2 >= cut);
        s.pending.insert(key.clone(), (site_id.to_string(), item_id.to_string(), now()));
    });
    json!([[{ "text": "✅ 上线", "callback_data": format!("a:{key}") }, { "text": "驳回", "callback_data": format!("d:{key}") }]])
}

// ───────────────────────── 收消息（长轮询） ─────────────────────────

/// 启动时调一次：有 Token 就一直长轮询；没有就隔一会儿再看。
pub fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        loop {
            let data_dir = app.state::<crate::AppState>().data_dir.clone();
            let Some(t) = token() else {
                tokio::time::sleep(Duration::from_secs(20)).await;
                continue;
            };
            let s = load(&data_dir);
            // 没绑定、也不在配对中：不用收消息
            if s.chat_id == 0 && s.pair_until < now() {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
            match call(&t, "getUpdates", json!({ "offset": s.offset, "timeout": 40, "allowed_updates": ["message", "callback_query"] }), 55).await {
                Ok(v) => {
                    if !s.last_err.is_empty() {
                        update(&data_dir, |s| s.last_err.clear());
                    }
                    for u in v.as_array().cloned().unwrap_or_default() {
                        let id = u["update_id"].as_i64().unwrap_or(0);
                        update(&data_dir, |s| s.offset = s.offset.max(id + 1));
                        handle(&app, &t, &u).await;
                    }
                }
                Err(e) => {
                    update(&data_dir, |s| s.last_err = e);
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
        }
    });
}

async fn handle(app: &tauri::AppHandle, token: &str, u: &Value) {
    use tauri::{Emitter, Manager};
    let data_dir = app.state::<crate::AppState>().data_dir.clone();
    let s = load(&data_dir);
    if let Some(m) = u.get("message") {
        let chat = m["chat"]["id"].as_i64().unwrap_or(0);
        let private = m["chat"]["type"].as_str() == Some("private");
        let text = m["text"].as_str().unwrap_or("").trim().to_string();
        // 配对：/start 123456 或直接发配对码
        if private && !s.pair_code.is_empty() && s.pair_until >= now() {
            let code = text.strip_prefix("/start").map(str::trim).unwrap_or(&text);
            if code == s.pair_code {
                let name = [m["chat"]["first_name"].as_str(), m["chat"]["username"].as_str().map(|u| u.trim_start_matches('@'))]
                    .into_iter()
                    .flatten()
                    .next()
                    .unwrap_or("")
                    .to_string();
                update(&data_dir, |s| {
                    s.chat_id = chat;
                    s.chat_name = name;
                    s.pair_code.clear();
                    s.pair_until = 0;
                    s.enabled = true;
                });
                let _ = send_to(token, chat, "已和 Cligc Pilot 绑定。托管日报、站点打不开、等你确认上线的改动会发到这里。\n发 /help 看能做什么。", None).await;
                let _ = app.emit("push-changed", ());
                return;
            }
        }
        if chat == 0 || chat != s.chat_id {
            return; // 不是绑定的那个人：不理
        }
        let reply = command(app, &text).await;
        if let Some((text, buttons)) = reply {
            let _ = send_to(token, chat, &text, None).await;
            for (t, b) in buttons {
                let _ = send_to(token, chat, &t, b).await;
            }
        }
    } else if let Some(q) = u.get("callback_query") {
        let qid = q["id"].as_str().unwrap_or("").to_string();
        let chat = q["message"]["chat"]["id"].as_i64().unwrap_or(0);
        if chat == 0 || chat != s.chat_id {
            return;
        }
        let data = q["data"].as_str().unwrap_or("");
        let answer = button(app, data).await;
        let _ = call(token, "answerCallbackQuery", json!({ "callback_query_id": qid, "text": clip(&answer, 190) }), 15).await;
        // 按钮点过了就去掉，消息末尾记一下结果
        if let (Some(mid), Some(old)) = (q["message"]["message_id"].as_i64(), q["message"]["text"].as_str()) {
            let _ = call(token, "editMessageText", json!({ "chat_id": chat, "message_id": mid, "text": clip(&format!("{old}\n\n→ {answer}"), 3900), "disable_web_page_preview": true }), 15).await;
        }
    }
}

/// 点了按钮：a:<键> 上线、d:<键> 驳回。返回给你看的一句话。
async fn button(app: &tauri::AppHandle, data: &str) -> String {
    use tauri::Manager;
    let st = app.state::<crate::AppState>();
    let Some((act, key)) = data.split_once(':') else { return "看不懂这个按钮".into() };
    let s = load(&st.data_dir);
    let Some((site_id, item_id, _)) = s.pending.get(key).cloned() else { return "这个按钮过期了，到电脑上的「待处理」里处理".into() };
    let Ok(site) = st.sites.get(&site_id) else { return "这个站点已经删掉了".into() };
    let Some(item) = crate::site_ops::load_queue(&st.sites.data_dir(&site_id)).into_iter().find(|i| i.id == item_id) else { return "这条改动不在了".into() };
    if item.status != "proposed" {
        return format!("这条已经处理过了（{}）", status_label(&item.status));
    }
    let ctx = crate::site_runner::Ctx { app: app.clone() };
    let r = match act {
        "a" if !s.allow_approve => Err("设置里关掉了「在手机上确认上线」".to_string()),
        "a" => crate::site_runner::start(ctx, &site_id, "accept", "manual", Some(item_id.clone())).map(|_| "开始合并上线，完成后会再发消息".to_string()),
        "d" => crate::site_runner::dismiss(&ctx, &site, &item_id).await.map(|_| "已驳回".to_string()),
        _ => Err("看不懂这个按钮".into()),
    };
    if r.is_ok() {
        update(&st.data_dir, |s| {
            s.pending.remove(key);
        });
    }
    r.unwrap_or_else(|e| format!("没成功：{e}"))
}

fn status_label(s: &str) -> &str {
    match s {
        "working" => "AI 正在处理",
        "merged" => "已合并",
        "deployed" => "已上线",
        "dismissed" => "已驳回",
        "rolled_back" => "已回滚",
        "failed" => "失败",
        "done" => "已处理",
        "open" => "待处理",
        other => other,
    }
}

type Reply = Option<(String, Vec<(String, Option<Value>)>)>;

/// 文字命令。返回 (回复, 之后再逐条发的消息)。
async fn command(app: &tauri::AppHandle, text: &str) -> Reply {
    use tauri::Manager;
    let st = app.state::<crate::AppState>();
    let cmd = text.split_whitespace().next().unwrap_or("").split('@').next().unwrap_or("").to_ascii_lowercase();
    match cmd.as_str() {
        "/status" | "状态" => Some((status_text(&st), Vec::new())),
        "/pending" | "待确认" => {
            let mut items = Vec::new();
            for site in st.sites.list() {
                for i in crate::site_ops::load_queue(&st.sites.data_dir(&site.id)).into_iter().filter(|i| i.status == "proposed") {
                    items.push((site.clone(), i));
                }
            }
            if items.is_empty() {
                return Some(("没有等你确认的改动。".into(), Vec::new()));
            }
            let allow = load(&st.data_dir).allow_approve;
            let n = items.len();
            let msgs = items
                .into_iter()
                .take(10)
                .map(|(site, i)| {
                    let b = (allow && !i.branch.is_empty()).then(|| buttons_for(&st.data_dir, &site.id, &i.id));
                    let mut t = proposal_text(&site.name, &i);
                    if i.branch.is_empty() {
                        t.push_str("（这条是方案，要在电脑上的会话里确认）");
                    }
                    (t, b)
                })
                .collect();
            Some((format!("{n} 条等你确认{}：", if n > 10 { "（先发最早的 10 条）" } else { "" }), msgs))
        }
        "/pause" | "暂停" => {
            let mut c = crate::managed::load(&st.data_dir);
            c.paused = true;
            let _ = crate::managed::save(&st.data_dir, &c);
            use tauri::Emitter;
            let _ = app.emit("managed-changed", ());
            Some(("已暂停全部托管。正在跑的任务会跑完，之后不再自动开始新的。发 /resume 恢复。".into(), Vec::new()))
        }
        "/resume" | "恢复" => {
            let mut c = crate::managed::load(&st.data_dir);
            c.paused = false;
            let _ = crate::managed::save(&st.data_dir, &c);
            use tauri::Emitter;
            let _ = app.emit("managed-changed", ());
            Some(("已恢复托管。".into(), Vec::new()))
        }
        "/start" | "/help" | "帮助" => Some((
            "可以发：\n/status 托管概况\n/pending 等你确认的改动（带上线 / 驳回按钮）\n/pause 暂停全部托管\n/resume 恢复托管\n\nPilot 要开着才能收到（关窗口没事，留在菜单栏就行）。".into(),
            Vec::new(),
        )),
        _ => Some(("没看懂。发 /help 看能做什么。".into(), Vec::new())),
    }
}

fn status_text(st: &crate::AppState) -> String {
    let cfg = crate::managed::load(&st.data_dir);
    let sites = st.sites.list();
    let managed: Vec<&crate::sites::Site> = sites.iter().filter(|s| cfg.group_of(&s.id).is_some()).collect();
    let mut proposed = 0;
    let mut failed = 0;
    let mut down = Vec::new();
    let mut flagged = Vec::new();
    for s in &sites {
        let dir = st.sites.data_dir(&s.id);
        let q = crate::site_ops::load_queue(&dir);
        proposed += q.iter().filter(|i| i.status == "proposed").count();
        failed += q.iter().filter(|i| i.status == "failed").count();
        if !s.domain.is_empty() && crate::monitor::load(&dir).down() {
            down.push(s.domain.clone());
        }
        if !s.domain.is_empty() && crate::safety::load(&dir).is_flagged() {
            flagged.push(s.domain.clone());
        }
    }
    let since = now().saturating_sub(86_400);
    let acts: Vec<crate::managed::Activity> = crate::managed::activity(&st.data_dir, 2000).into_iter().filter(|a| a.ts >= since).collect();
    let shipped = acts.iter().filter(|a| a.kind == "shipped").count();
    let mut t = format!(
        "托管 {} 个站点{}\n过去 24 小时 AI 上线 {shipped} 项\n等你确认 {proposed} 条{}\n",
        managed.len(),
        if cfg.paused { "（已暂停）" } else { "" },
        if failed > 0 { format!("，失败 {failed} 条") } else { String::new() }
    );
    if down.is_empty() {
        t.push_str("站点都能打开");
    } else {
        t.push_str(&format!("打不开：{}", down.join("、")));
    }
    if !flagged.is_empty() {
        t.push_str(&format!("\n被标记为危险网站：{}", flagged.join("、")));
    }
    t
}

// ───────────────────────── 设置（界面） ─────────────────────────

#[derive(Serialize)]
pub struct Status {
    pub has_token: bool,
    pub bot_name: String,
    pub chat_name: String,
    pub bound: bool,
    pub enabled: bool,
    pub allow_approve: bool,
    /// 配对中：配对码和到期时间
    pub pair_code: String,
    pub pair_until: u64,
    pub last_err: String,
}

fn status(data_dir: &Path) -> Status {
    let s = load(data_dir);
    let pairing = s.pair_until >= now();
    Status {
        has_token: token().is_some(),
        bot_name: s.bot_name,
        chat_name: s.chat_name,
        bound: s.chat_id != 0,
        enabled: s.enabled,
        allow_approve: s.allow_approve,
        pair_code: if pairing { s.pair_code } else { String::new() },
        pair_until: if pairing { s.pair_until } else { 0 },
        last_err: s.last_err,
    }
}

#[tauri::command]
pub fn push_status(state: tauri::State<'_, crate::AppState>) -> Status {
    status(&state.data_dir)
}

/// 填 Token：先 getMe 确认能用，再进钥匙串。换了机器人就要重新绑定。
#[tauri::command]
pub async fn push_set_token(state: tauri::State<'_, crate::AppState>, token: String) -> Result<Status, String> {
    let t = token.trim().to_string();
    if !valid_token(&t) {
        return Err("这不像机器人 Token（应该是 123456789:AA… 这样，在 @BotFather 里复制）".into());
    }
    let me = call(&t, "getMe", json!({}), 20).await?;
    let name = me["username"].as_str().unwrap_or("").to_string();
    crate::keychain::set_named_secret(SECRET, &t)?;
    update(&state.data_dir, |s| {
        if s.bot_name != name {
            s.chat_id = 0;
            s.chat_name.clear();
            s.offset = 0;
            s.pending.clear();
        }
        s.bot_name = name;
        s.last_err.clear();
    });
    Ok(status(&state.data_dir))
}

/// 开始配对：给一个 6 位配对码（10 分钟有效）
#[tauri::command]
pub fn push_pair(state: tauri::State<'_, crate::AppState>) -> Result<Status, String> {
    if token().is_none() {
        return Err("先填机器人 Token".into());
    }
    let n = u32::from_str_radix(&uuid::Uuid::new_v4().simple().to_string()[..8], 16).unwrap_or(0) % 900_000 + 100_000;
    update(&state.data_dir, |s| {
        s.pair_code = n.to_string();
        s.pair_until = now() + PAIR_SECS;
    });
    Ok(status(&state.data_dir))
}

#[tauri::command]
pub fn push_set(state: tauri::State<'_, crate::AppState>, enabled: Option<bool>, allow_approve: Option<bool>) -> Status {
    update(&state.data_dir, |s| {
        if let Some(e) = enabled {
            s.enabled = e;
        }
        if let Some(a) = allow_approve {
            s.allow_approve = a;
        }
    });
    status(&state.data_dir)
}

#[tauri::command]
pub async fn push_test(state: tauri::State<'_, crate::AppState>) -> Result<(), String> {
    let s = load(&state.data_dir);
    let t = token().ok_or("先填机器人 Token")?;
    if s.chat_id == 0 {
        return Err("还没绑定".into());
    }
    send_to(&t, s.chat_id, "这是一条测试消息：Cligc Pilot 的通知能发到这里。", None).await.map(|_| ())
}

/// 断开：删掉 Token、清掉绑定
#[tauri::command]
pub fn push_forget(state: tauri::State<'_, crate::AppState>) -> Result<Status, String> {
    let _ = crate::keychain::delete_named_secret(SECRET);
    update(&state.data_dir, |s| *s = Settings { allow_approve: true, ..Default::default() });
    Ok(status(&state.data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_shape() {
        assert!(valid_token("123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw0"));
        assert!(!valid_token("AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw0"));
        assert!(!valid_token("12a:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw0"));
        assert!(!valid_token("123:short"));
    }

    #[test]
    fn proposal_text_lists_summary_files_and_pr() {
        let mut i = crate::site_ops::QueueItem::default();
        i.opp.title = "改 /a/ 的标题".into();
        i.ai_summary = "把标题改成回应搜索意图的写法".into();
        i.files = (0..7).map(|n| format!("f{n}.md")).collect();
        i.pr_url = "https://github.com/x/y/pull/3".into();
        let t = proposal_text("站A", &i);
        assert!(t.starts_with("【站A】等你确认上线\n改 /a/ 的标题"));
        assert!(t.contains("f4.md 等 7 个文件") && t.contains("PR：https://github.com/x/y/pull/3"));
    }
}
