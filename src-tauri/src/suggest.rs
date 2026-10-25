//! 输入框的建议回复：每轮结束后，用和这个对话**同一家**的小模型猜一句用户接下来最可能发的话，
//! 放进输入框的占位文字（空着时按 Tab 填入，不自动发送）。
//!
//! - API 连接（brain = `api:<id>`）：同一个 API 连接里挑名字带 haiku / mini / flash / nano / lite / small 的模型，
//!   没有就用这个对话自己的模型。
//! - Claude Code（brain = `claude`）：`claude -p … --model haiku`（走用户自己的订阅）。
//! - 其他（codex / grok）：不猜。不把对话发给用户没选的那家。
//!
//! AI 原话里明确让用户回的那句（「回复我『好了』」）前端自己认，认出来就不调这里。

use serde::Serialize;
use std::path::Path;
use std::time::Duration;

/// 每段最多带这么多字给小模型（只看最后一轮的结尾）。
const USER_TAIL: usize = 1_200;
const AI_TAIL: usize = 2_400;

#[derive(Serialize, Default)]
pub struct Suggestion {
    /// 这条建议对应的 AI 回复（消息 ts）：前端据此判断建议还新不新鲜
    pub ts: u64,
    /// 空＝不给建议
    pub text: String,
}

fn tail(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

fn prompt(user: &str, ai: &str) -> String {
    format!(
        "下面是用户和 AI 助手一次对话的最后一轮。猜用户接下来最可能发给 AI 的那一条消息：\n\
- 用用户的语言和口吻，像用户自己打的字，越短越好（中文不超过 20 个字，英文不超过 12 个词）\n\
- 是用户会直接发出去的话，比如「好了」「继续」「部署上线」「标题改短一点」，不是对 AI 的评价、感谢或总结\n\
- AI 在等用户做完某件事时，就是用户做完后会说的那句\n\
只输出这条消息本身，不要引号、不要解释。猜不出、或者这一轮已经结束不需要用户再说什么，只输出 NONE。\n\n\
<用户>\n{}\n</用户>\n<AI>\n{}\n</AI>",
        tail(user, USER_TAIL),
        tail(ai, AI_TAIL)
    )
}

/// 模型输出 → 一句建议（去引号、只要第一行、太长或 NONE 就不要）。
pub fn clean(raw: &str) -> String {
    let line = raw.trim().lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let t = line.trim_matches(|c: char| "「」『』“”\"'`*".contains(c) || c.is_whitespace());
    if t.is_empty() || t.eq_ignore_ascii_case("none") || t.chars().count() > 40 {
        return String::new();
    }
    t.to_string()
}

fn small_model(models: &[String], fallback: &str) -> String {
    const SMALL: [&str; 6] = ["haiku", "mini", "flash", "nano", "lite", "small"];
    models.iter().find(|m| SMALL.iter().any(|s| m.to_ascii_lowercase().contains(s))).cloned().unwrap_or_else(|| fallback.to_string())
}

async fn via_api(data_dir: &Path, profile_id: &str, model: &str, prompt: &str) -> Result<String, String> {
    let loaded = crate::api_profiles::load_enabled(data_dir, profile_id)?;
    let p = &loaded.profile;
    let model = small_model(&p.models, if model.is_empty() { &p.default_model } else { model });
    let client = crate::api_profiles::http_client()?;
    let (route, body) = match p.protocol.as_str() {
        "anthropic_messages" => ("messages", serde_json::json!({ "model": model, "max_tokens": 80, "messages": [{ "role": "user", "content": prompt }] })),
        "openai_responses" => ("responses", serde_json::json!({ "model": model, "input": prompt, "max_output_tokens": 400 })),
        _ => ("chat/completions", serde_json::json!({ "model": model, "max_tokens": 80, "messages": [{ "role": "user", "content": prompt }] })),
    };
    let req = client.post(crate::api_profiles::endpoint(p, route)?).json(&body).timeout(Duration::from_secs(30));
    let req = if p.protocol == "anthropic_messages" {
        req.header("x-api-key", &loaded.api_key).header("anthropic-version", "2023-06-01")
    } else {
        req.bearer_auth(&loaded.api_key)
    };
    let r = req.send().await.map_err(|e| format!("建议回复：请求失败 {e}"))?;
    if !r.status().is_success() {
        return Err(format!("建议回复：HTTP {}", r.status().as_u16()));
    }
    let v: serde_json::Value = r.json().await.map_err(|_| "建议回复：响应看不懂".to_string())?;
    let text = match p.protocol.as_str() {
        "anthropic_messages" => v["content"].as_array().and_then(|a| a.iter().find_map(|c| c["text"].as_str())).unwrap_or("").to_string(),
        "openai_responses" => v["output"]
            .as_array()
            .map(|o| o.iter().flat_map(|i| i["content"].as_array().cloned().unwrap_or_default()).filter_map(|c| c["text"].as_str().map(String::from)).collect::<String>())
            .unwrap_or_default(),
        _ => v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string(),
    };
    Ok(text)
}

async fn via_claude(prompt: &str) -> Result<String, String> {
    let path = crate::brains::resolve_bin("claude");
    if path.is_empty() {
        return Err("没有 claude".into());
    }
    let mut cmd = tokio::process::Command::new(&path);
    if let Ok(Some(token)) = crate::keychain::claude_oauth_token() {
        cmd.env_remove("ANTHROPIC_AUTH_TOKEN").env_remove("ANTHROPIC_API_KEY").env("CLAUDE_CODE_OAUTH_TOKEN", token);
    }
    // 在临时目录里跑：不读任何项目的 CLAUDE.md / 设置
    cmd.args(["-p", prompt, "--model", "haiku"])
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = tokio::time::timeout(Duration::from_secs(40), cmd.output()).await.map_err(|_| "建议回复：claude 超时".to_string())?.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("建议回复：claude 失败".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 猜一句。拿不到（这家不支持、没配好、网络）就返回空建议，不报错打扰用户。
pub async fn suggest(data_dir: &Path, conv: &crate::convo::Conversation) -> Suggestion {
    let visible: Vec<&crate::convo::Message> = conv.messages.iter().filter(|m| !m.hidden).collect();
    let Some(ai) = visible.last().filter(|m| m.role == "assistant" && !m.error && !m.text.trim().is_empty()) else { return Suggestion::default() };
    let user = visible.iter().rev().find(|m| m.role == "user").map(|m| m.text.as_str()).unwrap_or("");
    let p = prompt(user, &ai.text);
    let raw = if let Some(id) = conv.brain.strip_prefix("api:") {
        via_api(data_dir, id, &conv.model, &p).await
    } else if conv.brain == "claude" {
        via_claude(&p).await
    } else {
        return Suggestion { ts: ai.ts, text: String::new() };
    };
    Suggestion { ts: ai.ts, text: raw.map(|r| clean(&r)).unwrap_or_default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_model_output() {
        assert_eq!(clean("「好了」"), "好了");
        assert_eq!(clean("  继续\n（解释）"), "继续");
        assert_eq!(clean("NONE"), "");
        assert_eq!(clean("none."), "none.");
        assert_eq!(clean(""), "");
        assert_eq!(clean(&"很长".repeat(30)), "");
    }

    #[test]
    fn picks_small_model() {
        let ms = vec!["claude-opus-5-5".to_string(), "claude-haiku-5-5".to_string()];
        assert_eq!(small_model(&ms, "claude-opus-5-5"), "claude-haiku-5-5");
        assert_eq!(small_model(&["gpt-5".to_string()], "gpt-5"), "gpt-5");
        assert_eq!(small_model(&[], "x"), "x");
    }

    #[test]
    fn prompt_keeps_only_the_tail() {
        let long = "前".repeat(5000) + "最后一句";
        let p = prompt("你好", &long);
        assert!(p.contains("最后一句") && p.chars().count() < AI_TAIL + USER_TAIL + 600);
    }
}
