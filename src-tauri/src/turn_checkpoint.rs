//! 运行中的一轮边跑边存：把发给界面的事件流照抄一份，攒出「写到一半的回复」，每隔几秒存进会话。
//!
//! 为什么要：回复原本只在这一轮结束时才落库。应用中途退出（关掉、崩溃、开发时重启）时，
//! 这一轮做过的事——几分钟的正文和工具步骤——全丢，对话停在用户那条消息上。存下来之后，
//! 下次启动由 ConvStore::mark_idle 把它转成一条「被中断」的回复，前面做到哪一目了然。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::ipc::{Channel, InvokeResponseBody};

use crate::agent::TurnEvent;
use crate::convo::{utf16_len, ConvStore, Message, ToolCall};

const EVERY: Duration = Duration::from_secs(3);

#[derive(Default)]
struct Draft {
    text: String,
    tools: Vec<ToolCall>,
    dirty: bool,
    saved: Option<Instant>,
}

/// 包一层界面通道：事件照常转给界面，同时攒草稿、定期存盘。
pub fn wrap(store: ConvStore, conv_id: String, ui: Channel<TurnEvent>) -> Channel<TurnEvent> {
    let draft = Arc::new(Mutex::new(Draft::default()));
    Channel::new(move |body: InvokeResponseBody| {
        let InvokeResponseBody::Json(json) = body else { return Ok(()) };
        let Ok(ev) = serde_json::from_str::<TurnEvent>(&json) else { return Ok(()) };
        let snapshot = {
            let mut d = draft.lock().unwrap();
            match &ev {
                TurnEvent::Delta { text } => {
                    d.text.push_str(text);
                    d.dirty = true;
                }
                TurnEvent::TextReset { text } => {
                    d.text = text.clone();
                    d.dirty = true;
                }
                TurnEvent::Tool { label, detail } => {
                    let at = utf16_len(&d.text);
                    d.tools.push(ToolCall { label: label.clone(), detail: detail.clone(), at });
                    d.dirty = true;
                }
                _ => {}
            }
            let due = d.saved.map(|t| t.elapsed() >= EVERY).unwrap_or(true);
            if d.dirty && due && !matches!(ev, TurnEvent::Done { .. }) {
                d.dirty = false;
                d.saved = Some(Instant::now());
                Some((d.text.clone(), d.tools.clone()))
            } else {
                None
            }
        };
        if let Some((text, tools)) = snapshot {
            let _ = store.checkpoint(
                &conv_id,
                Message { role: "assistant".into(), text, tools, ts: crate::now_secs(), hidden: false, error: false, limit_reset: None, partial: true },
            );
        }
        // 界面那头关了（窗口关掉）也不影响这一轮继续跑
        let _ = ui.send(ev);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convo::Conversation;

    #[test]
    fn forwards_every_event_and_saves_a_draft_of_the_reply() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConvStore::new(dir.path());
        let c: Conversation = serde_json::from_value(serde_json::json!({
            "id": "t", "conn_id": "c", "conn_name": "", "site_slug": "", "site_name": "", "task_type": "site",
            "brain": "claude", "model": "m", "session_ref": "", "title": "t",
            "messages": [{"role": "user", "text": "做", "ts": 0}], "status": "running", "created_at": 0, "updated_at": 0
        }))
        .unwrap();
        store.upsert(c).unwrap();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let s2 = seen.clone();
        let ui = Channel::<TurnEvent>::new(move |b| {
            if let InvokeResponseBody::Json(j) = b {
                s2.lock().unwrap().push(j);
            }
            Ok(())
        });
        let ch = wrap(store.clone(), "t".into(), ui);
        ch.send(TurnEvent::Delta { text: "先规划".into() }).unwrap();
        ch.send(TurnEvent::Tool { label: "Write".into(), detail: "src/index.astro".into() }).unwrap();
        ch.send(TurnEvent::Delta { text: "，再写页面".into() }).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 3, "界面照常收到每个事件");
        // 第一次就存（之后每 3 秒最多一次）：磁盘上有草稿
        store.mark_idle(1);
        let got = store.get("t").unwrap();
        let last = got.messages.last().unwrap();
        assert!(last.text.contains("先规划"), "{}", last.text);
    }
}
