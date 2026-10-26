//! API 执行器每一轮的停止规则：按「有没有进展」判断，而不是数次数。
//!
//! - 正常干活（写新文件、跑不同的命令、读不同的内容）不打断，整站重做这种几百步的活也能一轮做完。
//! - 真的卡住才停：同一个操作带着同样的参数、得到同样的结果反复出现（原地打转），或者连续很多次工具调用都失败。
//! - `max_requests` 只是最后防线（防极端失控的费用），正常任务碰不到。
//! - 用户随时可以手动停止。
use std::collections::VecDeque;
use std::fmt;
use std::hash::{Hash, Hasher};

/// 最后防线：一轮最多请求模型多少次。
pub(crate) const MAX_REQUESTS: usize = 400;
/// 最近多少次工具调用里找重复。
const LOOP_WINDOW: usize = 20;
/// 同一个「操作 + 参数 + 结果」在窗口里出现几次算原地打转。
const LOOP_REPEATS: usize = 3;
/// 同一个操作带着同样的参数连续失败多少次停下。
const ERROR_STREAK: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct ApiExecutionPolicy {
    pub(crate) max_requests: usize,
}

impl Default for ApiExecutionPolicy {
    fn default() -> Self {
        Self { max_requests: MAX_REQUESTS }
    }
}

impl ApiExecutionPolicy {
    pub(crate) fn check_boundary(&self, requests: usize) -> Result<(), StopReason> {
        if requests >= self.max_requests {
            return Err(StopReason::RequestBudget {
                limit: self.max_requests,
            });
        }
        Ok(())
    }
}

/// 看 AI 有没有在原地打转：记录最近的工具调用（名字、参数、结果的指纹）。
///
/// 结果指纹只看工具输出。read_file / view_image 的输出里带着文件的修改时间和大小
/// （见 api_agent_tools），所以「截图到同一个文件 → 看图 → 改 → 再截 → 再看」这种正常循环，
/// 每次看到的是不同的指纹，不会被当成卡住。
#[derive(Default)]
pub(crate) struct ProgressGuard {
    recent: VecDeque<(u64, String)>,
    /// 连续失败：（最近一次失败的「操作 + 参数」指纹，连续次数）。换了命令就重新数，
    /// 不然 grep 没匹配到、逐个修失败的测试这种各不相同的失败也会被数进去。
    errors: Option<(u64, usize)>,
}

fn call_fingerprint(name: &str, args: &serde_json::Value) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    args.to_string().hash(&mut h);
    h.finish()
}

fn fingerprint(name: &str, args: &serde_json::Value, output: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    args.to_string().hash(&mut h);
    // 结果只取开头一段：同样的失败输出、同样的文件内容都会一样
    output.chars().take(4000).collect::<String>().hash(&mut h);
    h.finish()
}

impl ProgressGuard {
    /// 每执行完一次工具调用记一笔；判断为卡住就返回停止原因。
    pub(crate) fn observe(&mut self, name: &str, args: &serde_json::Value, output: &str, failed: bool) -> Result<(), StopReason> {
        let fp = fingerprint(name, args, output);
        self.recent.push_back((fp, name.to_string()));
        if self.recent.len() > LOOP_WINDOW {
            self.recent.pop_front();
        }
        let times = self.recent.iter().filter(|(f, _)| *f == fp).count();
        if times >= LOOP_REPEATS {
            return Err(StopReason::Looping { tool: name.to_string(), times });
        }
        if !failed {
            self.errors = None;
            return Ok(());
        }
        let call = call_fingerprint(name, args);
        let streak = match self.errors {
            Some((last, n)) if last == call => n + 1,
            _ => 1,
        };
        self.errors = Some((call, streak));
        if streak >= ERROR_STREAK {
            return Err(StopReason::ErrorStreak { times: streak });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StopReason {
    RequestBudget { limit: usize },
    /// 同一个操作、同样的参数、同样的结果反复出现
    Looping { tool: String, times: usize },
    /// 连续很多次工具调用都失败
    ErrorStreak { times: usize },
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestBudget { limit } => write!(f, "这一轮已经连续工作了 {limit} 步，先停一下（防止意外失控产生大量费用）。回复「继续」可以接着做，已经做过的步骤 AI 能看到，不会从头再来"),
            Self::Looping { tool, times } => write!(f, "AI 好像卡住了：同一个操作（{tool}）用同样的参数做了 {times} 次、结果都一样，先停在这里。看一下最后几步卡在哪，给它一点提示后回复「继续」"),
            Self::ErrorStreak { times } => write!(f, "同一个操作连续 {times} 次都失败了，先停在这里。看一下最后几步的错误，给它一点提示后回复「继续」"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ceiling_is_only_a_last_resort() {
        let policy = ApiExecutionPolicy::default();
        assert_eq!(policy.max_requests, MAX_REQUESTS);
        assert!(policy.check_boundary(MAX_REQUESTS - 1).is_ok());
        assert_eq!(policy.check_boundary(MAX_REQUESTS), Err(StopReason::RequestBudget { limit: MAX_REQUESTS }));
    }

    #[test]
    fn real_work_is_never_stopped() {
        // 几百次不同的操作（写不同的文件、跑构建且结果在变）都不算卡住
        let mut g = ProgressGuard::default();
        for i in 0..500 {
            assert!(g.observe("write_file", &json!({"path": format!("src/content/p{i}.md")}), "ok", false).is_ok());
            if i % 10 == 0 {
                assert!(g.observe("run_command", &json!({"cmd": "npm run build"}), &format!("built {i} pages"), false).is_ok());
            }
        }
    }

    #[test]
    fn same_call_same_result_three_times_is_a_loop() {
        let mut g = ProgressGuard::default();
        let args = json!({"cmd": "npm run build"});
        assert!(g.observe("run_command", &args, "Error: x is not defined", true).is_ok());
        assert!(g.observe("read_file", &json!({"path": "a"}), "...", false).is_ok());
        assert!(g.observe("run_command", &args, "Error: x is not defined", true).is_ok());
        assert_eq!(g.observe("run_command", &args, "Error: x is not defined", true), Err(StopReason::Looping { tool: "run_command".into(), times: 3 }));
    }

    #[test]
    fn repeats_far_apart_are_fine() {
        // 隔了很多步再读同一个文件是正常的（比如改完回头确认）
        let mut g = ProgressGuard::default();
        for round in 0..5 {
            assert!(g.observe("read_file", &json!({"path": "astro.config.mjs"}), "same", false).is_ok());
            for i in 0..LOOP_WINDOW {
                assert!(g.observe("write_file", &json!({"path": format!("r{round}/{i}")}), "ok", false).is_ok());
            }
        }
    }

    #[test]
    fn long_error_streak_stops() {
        // 同一条命令（参数一样）连续失败、每次错误不同（不算原地打转）：到上限就停
        let mut g = ProgressGuard::default();
        let args = json!({"cmd": "npm test"});
        for i in 0..ERROR_STREAK - 1 {
            assert!(g.observe("run_command", &args, &format!("fail {i}"), true).is_ok());
        }
        // 中间成功一次就重新计数
        assert!(g.observe("run_command", &json!({"cmd": "ok"}), "fine", false).is_ok());
        for i in 0..ERROR_STREAK - 1 {
            assert!(g.observe("run_command", &args, &format!("again {i}"), true).is_ok());
        }
        assert_eq!(g.observe("run_command", &args, "last", true), Err(StopReason::ErrorStreak { times: ERROR_STREAK }));
    }

    #[test]
    fn different_failing_commands_are_not_a_streak() {
        // grep 没匹配到、逐个修失败的测试：各不相同的失败不算卡住
        let mut g = ProgressGuard::default();
        for i in 0..(ERROR_STREAK * 3) {
            assert!(g.observe("run_command", &json!({"cmd": format!("grep -r needle{i} src")}), "exit 1", true).is_ok());
        }
        // 同一条命令失败几次、换一条、再回来：换命令就重新数
        let a = json!({"cmd": "npm test"});
        for i in 0..ERROR_STREAK - 1 {
            assert!(g.observe("run_command", &a, &format!("fail {i}"), true).is_ok());
        }
        assert!(g.observe("run_command", &json!({"cmd": "other"}), "fail", true).is_ok());
        for i in 0..ERROR_STREAK - 1 {
            assert!(g.observe("run_command", &a, &format!("later {i}"), true).is_ok());
        }
    }

    #[test]
    fn viewing_a_rewritten_screenshot_is_progress() {
        // 截图到同一个文件再看：输出里带着修改时间和大小，每次指纹不同，不算原地打转
        let mut g = ProgressGuard::default();
        let args = json!({"path": "shot.png"});
        for stamp in 0..6 {
            let out = json!({"opened": "shot.png", "width": 1280, "height": 800, "modified": 1000 + stamp, "size": 5000 + stamp}).to_string();
            assert!(g.observe("view_image", &args, &out, false).is_ok());
            assert!(g.observe("write_file", &json!({"path": "src/x.astro", "content": format!("v{stamp}")}), "ok", false).is_ok());
            assert!(g.observe("run_command", &json!({"cmd": "shot"}), &format!("saved {stamp}"), false).is_ok());
        }
        // 文件没变（时间和大小一样）才算：第 3 次停
        let same = json!({"opened": "shot.png", "modified": 1, "size": 2}).to_string();
        assert!(g.observe("view_image", &args, &same, false).is_ok());
        assert!(g.observe("view_image", &args, &same, false).is_ok());
        assert!(g.observe("view_image", &args, &same, false).is_err());
    }
}
