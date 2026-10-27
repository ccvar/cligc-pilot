//! TypeSafe 的判断模型（Jev）：给一段内容和几个问题，返回带概率的类型化答案（选一个 / 是否成立 / 按等级打分），
//! 不生成文字。Pilot 拿它做几件规则做不好、又不值得调一次大模型的小判断，见 jev_checks。
//!
//! - API Key 只存钥匙串；配置（用哪个模型、哪些功能开着、本月用量）存在数据目录的 typesafe.json。
//! - 没配 Key、功能关着、或者调用失败：调用方一律按原来的规则走——Jev 只能让判断更严，不能放宽。
//! - 接口：POST https://api.typesafe.ai/v1/systemone（Bearer），GET /v1/models 用来测 Key（不计费）。

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

const API: &str = "https://api.typesafe.ai/v1";
const KEY_ACCOUNT: &str = "typesafe.api_key";
pub const DEFAULT_MODEL: &str = "jev-latest";
/// 每百万输入 token 的价格（美元，输出不计费），见 docs.typesafe.ai/models。只用来在设置里估算本月花费。
const USD_PER_MTOK: f64 = 0.042;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub verified_at: u64,
    /// 用量按月累计（YYYY-MM），换月清零
    #[serde(default)]
    pub month: String,
    #[serde(default)]
    pub month_tokens: u64,
}
fn default_model() -> String {
    DEFAULT_MODEL.into()
}
impl Default for Config {
    fn default() -> Self {
        Self { model: default_model(), verified_at: 0, month: String::new(), month_tokens: 0 }
    }
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("typesafe.json")
}
pub fn load(data_dir: &Path) -> Config {
    crate::store_file::read_or_quarantine(&config_path(data_dir))
}
fn save(data_dir: &Path, c: &Config) -> Result<(), String> {
    crate::sites::write_json(&config_path(data_dir), c, true)
}
/// 读改写配置时串行（用量累计可能和设置页保存同时发生）
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn update(data_dir: &Path, f: impl FnOnce(&mut Config)) -> Result<Config, String> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = load(data_dir);
    f(&mut c);
    save(data_dir, &c)?;
    Ok(c)
}

fn month_now() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

/// 一个能用的 Jev 客户端（有 Key、对应功能开着）。
pub struct Jev {
    key: String,
    pub model: String,
    data_dir: PathBuf,
}

fn app_data_dir() -> Option<PathBuf> {
    use tauri::Manager;
    crate::APP.get().map(|a| a.state::<crate::AppState>().data_dir.clone())
}

/// 能用的 Jev 客户端：没配 Key、不在应用里（单元测试）都返回 None。用不用、用在哪由调用方按托管方案的规则决定。
pub fn client() -> Option<Jev> {
    let data_dir = app_data_dir()?;
    let c = load(&data_dir);
    let key = crate::keychain::get_named_secret(KEY_ACCOUNT).ok().filter(|k| !k.trim().is_empty())?;
    Some(Jev { key, model: c.model, data_dir })
}

/// TypeSafe 返回的错误翻成人话（Key 不出现在任何报错里）。
fn http_error(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| match &v["detail"] {
            Value::String(s) => Some(s.clone()),
            Value::Array(a) => Some(a.iter().filter_map(|d| d["msg"].as_str()).collect::<Vec<_>>().join("；")),
            _ => None,
        })
        .unwrap_or_default();
    match status {
        401 | 403 => "TypeSafe 不认这个 API Key（填错了或已失效），到「设置 → 判断模型」里换一个".into(),
        429 => "TypeSafe 请求太频繁（超过账号的速率限制），稍后再试".into(),
        402 => "TypeSafe 账号余额不足或没开通计费".into(),
        400 | 422 => format!("TypeSafe 说请求不对：{}", if detail.is_empty() { body.chars().take(200).collect() } else { detail }),
        s if s >= 500 => format!("TypeSafe 服务出错（HTTP {s}），稍后再试"),
        s => format!("TypeSafe 返回 HTTP {s}：{}", if detail.is_empty() { body.chars().take(200).collect() } else { detail }),
    }
}

impl Jev {
    /// 问一组问题：`state` 是所有问题共同参考的内容，`questions` 是 {名字: 问题}。返回 {名字: 答案}。
    pub async fn ask(&self, state: Value, questions: Value) -> Result<Map<String, Value>, String> {
        let body = json!({ "model": self.model, "state": state, "questions": questions });
        let resp = crate::cf::http()
            .post(format!("{API}/systemone"))
            .bearer_auth(&self.key)
            .timeout(Duration::from_secs(60))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("连不上 TypeSafe：{}", if e.is_timeout() { "超时".to_string() } else { e.to_string() }))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| format!("读取 TypeSafe 返回失败：{e}"))?;
        if !(200..300).contains(&status) {
            return Err(http_error(status, &text));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("TypeSafe 返回的不是 JSON：{e}"))?;
        if let Some(n) = v["usage"]["input_tokens"].as_u64() {
            let month = month_now();
            let _ = update(&self.data_dir, |c| {
                if c.month != month {
                    c.month = month.clone();
                    c.month_tokens = 0;
                }
                c.month_tokens += n;
            });
        }
        v["answers"].as_object().cloned().ok_or_else(|| "TypeSafe 的返回里没有 answers".to_string())
    }
}

/// 从答案里取 Score 的分数（0 到等级数-1）。
pub fn score(answers: &Map<String, Value>, id: &str) -> Option<f64> {
    answers.get(id)?.get("score")?.as_f64()
}
/// 从答案里取 Noul 的「是」的概率（0 到 1）。
pub fn noul(answers: &Map<String, Value>, id: &str) -> Option<f64> {
    answers.get(id)?.get("noul")?.as_f64()
}

async fn list_models(key: &str) -> Result<Vec<String>, String> {
    let resp = crate::cf::http()
        .get(format!("{API}/models"))
        .bearer_auth(key)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("连不上 TypeSafe：{e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(http_error(status, &text));
    }
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    // 兼容 {data:[{id}]} / {models:[...]} / [...] 几种写法
    let arr = v["data"].as_array().or_else(|| v["models"].as_array()).or_else(|| v.as_array()).cloned().unwrap_or_default();
    Ok(arr.iter().filter_map(|m| m.as_str().map(str::to_string).or_else(|| m["id"].as_str().map(str::to_string))).collect())
}

// ---------- 设置页 ----------

#[derive(Serialize)]
pub struct Status {
    configured: bool,
    key_prefix: String,
    model: String,
    verified_at: u64,
    month_tokens: u64,
    month_cost_usd: f64,
}

fn status_of(data_dir: &Path) -> Status {
    let c = load(data_dir);
    let key = crate::keychain::get_named_secret(KEY_ACCOUNT).ok().filter(|k| !k.trim().is_empty());
    let tokens = if c.month == month_now() { c.month_tokens } else { 0 };
    Status {
        configured: key.is_some(),
        key_prefix: key.as_deref().map(crate::keychain::key_prefix).unwrap_or_default(),
        model: c.model,
        verified_at: c.verified_at,
        month_tokens: tokens,
        month_cost_usd: tokens as f64 / 1_000_000.0 * USD_PER_MTOK,
    }
}

#[tauri::command]
pub fn typesafe_status(state: tauri::State<'_, crate::AppState>) -> Status {
    status_of(&state.data_dir)
}

/// 保存设置。`key` 非空时先用它读一次模型列表确认有效，再存进钥匙串；留空＝沿用已存的 Key。
#[tauri::command]
pub async fn typesafe_save(state: tauri::State<'_, crate::AppState>, key: String, model: String) -> Result<Status, String> {
    let key = key.trim().to_string();
    let mut verified = 0;
    if !key.is_empty() {
        list_models(&key).await?;
        crate::keychain::set_named_secret(KEY_ACCOUNT, &key)?;
        verified = crate::now_secs();
    } else if crate::keychain::get_named_secret(KEY_ACCOUNT).map(|k| k.trim().is_empty()).unwrap_or(true) {
        return Err("先填 TypeSafe 的 API Key".into());
    }
    let model = if model.trim().is_empty() { DEFAULT_MODEL.to_string() } else { model.trim().to_string() };
    update(&state.data_dir, |c| {
        c.model = model;
        if verified > 0 {
            c.verified_at = verified;
        }
    })?;
    Ok(status_of(&state.data_dir))
}

/// 测试连接：只读模型列表，不计费。返回给人看的一句话。
#[tauri::command]
pub async fn typesafe_test(state: tauri::State<'_, crate::AppState>) -> Result<String, String> {
    let key = crate::keychain::get_named_secret(KEY_ACCOUNT).ok().filter(|k| !k.trim().is_empty()).ok_or("还没填 TypeSafe 的 API Key")?;
    let models = list_models(&key).await?;
    update(&state.data_dir, |c| c.verified_at = crate::now_secs())?;
    Ok(if models.is_empty() { "连接正常".into() } else { format!("连接正常，可用模型：{}", models.join("、")) })
}

#[tauri::command]
pub fn typesafe_remove(state: tauri::State<'_, crate::AppState>) -> Result<Status, String> {
    crate::keychain::delete_named_secret(KEY_ACCOUNT)?;
    update(&state.data_dir, |c| c.verified_at = 0)?;
    Ok(status_of(&state.data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_never_echo_the_key_and_explain_the_common_cases() {
        assert!(http_error(401, "{\"detail\":\"bad token sk-abc\"}").contains("不认这个 API Key"));
        assert!(!http_error(401, "{\"detail\":\"bad token sk-abc\"}").contains("sk-abc"));
        assert!(http_error(429, "").contains("太频繁"));
        assert!(http_error(422, "{\"detail\":[{\"loc\":[\"body\",\"state\"],\"msg\":\"Field required\",\"type\":\"missing\"}]}").contains("Field required"));
        assert!(http_error(503, "").contains("稍后再试"));
    }

    #[test]
    fn answers_are_read_by_question_name() {
        let a: Map<String, Value> = serde_json::from_value(json!({
            "depth": {"type": "score", "score": 2.4, "confidence": 0.6},
            "made_up": {"type": "noul", "noul": 0.12}
        }))
        .unwrap();
        assert_eq!(score(&a, "depth"), Some(2.4));
        assert_eq!(noul(&a, "made_up"), Some(0.12));
        assert_eq!(score(&a, "missing"), None);
    }

    #[test]
    fn config_defaults_and_old_files_still_load() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.model, DEFAULT_MODEL);
        // 早期版本里的全局开关字段直接忽略（用不用改由托管方案决定）
        let old: Config = serde_json::from_str("{\"model\":\"jev-1.14.0\",\"use_gate\":false,\"use_rank\":true}").unwrap();
        assert_eq!(old.model, "jev-1.14.0");
    }
}
