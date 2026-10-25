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

/// 猜一句。拿不到（这家不支持、没配好、网络）就返回空建议，不报错打扰用户。
pub async fn suggest(data_dir: &Path, conv: &crate::convo::Conversation) -> Suggestion {
    let visible: Vec<&crate::convo::Message> = conv.messages.iter().filter(|m| !m.hidden).collect();
    let Some(ai) = visible.last().filter(|m| m.role == "assistant" && !m.error && !m.text.trim().is_empty()) else { return Suggestion::default() };
    let user = visible.iter().rev().find(|m| m.role == "user").map(|m| m.text.as_str()).unwrap_or("");
    let p = prompt(user, &ai.text);
    if !(conv.brain.starts_with("api:") || conv.brain == "claude") {
        return Suggestion { ts: ai.ts, text: String::new() };
    }
    let ask = crate::oneshot::Ask { brain: &conv.brain, model: &conv.model, prompt: &p, max_tokens: 80, small: true, timeout: Duration::from_secs(40) };
    let raw = crate::oneshot::complete(data_dir, &ask).await;
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
    fn prompt_keeps_only_the_tail() {
        let long = "前".repeat(5000) + "最后一句";
        let p = prompt("你好", &long);
        assert!(p.contains("最后一句") && p.chars().count() < AI_TAIL + USER_TAIL + 600);
    }
}
