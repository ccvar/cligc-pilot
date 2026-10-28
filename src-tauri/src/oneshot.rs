//! 不开对话、直接问一次模型（输入框的建议回复、分发用的摘要都用它）。只用用户已经选定的那一家：
//! - `api:<id>`：同一个 API 连接（Messages / Responses / Chat Completions 三种协议）
//! - `claude`：`claude -p`（走用户自己的订阅）
//! 其他（codex / grok）不支持，返回错误，调用方自己决定怎么降级。

use std::path::Path;
use std::time::Duration;

/// 名字带 haiku / mini / flash / nano / lite / small 的模型，没有就用 fallback。
pub fn small_model(models: &[String], fallback: &str) -> String {
    const SMALL: [&str; 6] = ["haiku", "mini", "flash", "nano", "lite", "small"];
    models.iter().find(|m| SMALL.iter().any(|s| m.to_ascii_lowercase().contains(s))).cloned().unwrap_or_else(|| fallback.to_string())
}

pub struct Ask<'a> {
    /// 对话 / 站点用的模型服务：`api:<id>` | `claude` | …
    pub brain: &'a str,
    /// 对话 / 站点用的模型（空＝默认）
    pub model: &'a str,
    pub prompt: &'a str,
    pub max_tokens: u32,
    /// 用同一家的小模型（便宜、快）；否则用 `model`
    pub small: bool,
    pub timeout: Duration,
}

/// 问一次，返回模型的原文。
pub async fn complete(data_dir: &Path, ask: &Ask<'_>) -> Result<String, String> {
    if let Some(id) = ask.brain.strip_prefix("api:") {
        via_api(data_dir, id, ask).await
    } else if ask.brain == "claude" {
        via_claude(ask).await
    } else {
        Err(format!("{} 不支持直接问一次模型：请把模型服务换成 Claude Code 或 API 连接", if ask.brain.is_empty() { "这个模型服务" } else { ask.brain }))
    }
}

async fn via_api(data_dir: &Path, profile_id: &str, ask: &Ask<'_>) -> Result<String, String> {
    let loaded = crate::api_profiles::load_enabled(data_dir, profile_id)?;
    let p = &loaded.profile;
    let base = if ask.model.is_empty() { p.default_model.as_str() } else { ask.model };
    let model = if ask.small { small_model(&p.models, base) } else { base.to_string() };
    let client = crate::api_profiles::http_client()?;
    let (route, body) = match p.protocol.as_str() {
        "anthropic_messages" => ("messages", serde_json::json!({ "model": model, "max_tokens": ask.max_tokens, "messages": [{ "role": "user", "content": ask.prompt }] })),
        // 推理模型会先花掉一部分输出额度在思考上，给足
        "openai_responses" => ("responses", serde_json::json!({ "model": model, "input": ask.prompt, "max_output_tokens": ask.max_tokens.max(400) * 2 })),
        _ => ("chat/completions", serde_json::json!({ "model": model, "max_tokens": ask.max_tokens, "messages": [{ "role": "user", "content": ask.prompt }] })),
    };
    let req = client.post(crate::api_profiles::endpoint(p, route)?).json(&body).timeout(ask.timeout);
    let req = if p.protocol == "anthropic_messages" {
        req.header("x-api-key", &loaded.api_key).header("anthropic-version", "2023-06-01")
    } else {
        req.bearer_auth(&loaded.api_key)
    };
    let r = req.send().await.map_err(|e| if e.is_timeout() { "模型服务超时".to_string() } else { format!("请求模型服务失败：{e}") })?;
    if !r.status().is_success() {
        return Err(format!("模型服务返回 HTTP {}", r.status().as_u16()));
    }
    let v: serde_json::Value = r.json().await.map_err(|_| "模型服务的响应看不懂".to_string())?;
    let text = match p.protocol.as_str() {
        "anthropic_messages" => v["content"].as_array().map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect::<String>()).unwrap_or_default(),
        "openai_responses" => v["output"]
            .as_array()
            .map(|o| o.iter().flat_map(|i| i["content"].as_array().cloned().unwrap_or_default()).filter_map(|c| c["text"].as_str().map(String::from)).collect::<String>())
            .unwrap_or_default(),
        _ => v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string(),
    };
    Ok(text)
}

async fn via_claude(ask: &Ask<'_>) -> Result<String, String> {
    let path = crate::brains::resolve_bin("claude");
    if path.is_empty() {
        return Err("没有找到 claude".into());
    }
    let mut cmd = tokio::process::Command::new(&path);
    if let Ok(Some(token)) = crate::keychain::claude_oauth_token() {
        cmd.env_remove("ANTHROPIC_AUTH_TOKEN").env_remove("ANTHROPIC_API_KEY").env("CLAUDE_CODE_OAUTH_TOKEN", token);
    }
    let model = if ask.small { "haiku" } else if ask.model.is_empty() { crate::site_runner::SONNET } else { ask.model };
    // 在临时目录里跑：不读任何项目的 CLAUDE.md / 设置
    cmd.args(["-p", ask.prompt, "--model", model])
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
    let out = tokio::time::timeout(ask.timeout, cmd.output()).await.map_err(|_| "claude 超时".to_string())?.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("claude 没有正常返回（可能需要重新登录）".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_small_model() {
        let ms = vec!["claude-opus-5-5".to_string(), "claude-haiku-5-5".to_string()];
        assert_eq!(small_model(&ms, "claude-opus-5-5"), "claude-haiku-5-5");
        assert_eq!(small_model(&["gpt-5".to_string()], "gpt-5"), "gpt-5");
        assert_eq!(small_model(&[], "x"), "x");
    }
}
