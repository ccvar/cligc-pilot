//! Explicit per-turn API execution budget: one turn may issue at most
//! `max_requests` model requests (tool rounds included), then stops.
use std::fmt;

#[derive(Clone, Debug)]
pub(crate) struct ApiExecutionPolicy {
    pub(crate) max_requests: usize,
    /// 无人值守的运营任务：在 Pilot 准备的独立工作树里，不在危险清单上的命令（构建、检查、git 提交）
    /// 直接执行，和 Claude CLI 自动档同一条规则；危险命令照样要确认（无人值守时一律被拒绝）。
    pub(crate) safe_commands: bool,
}

impl Default for ApiExecutionPolicy {
    fn default() -> Self {
        Self { max_requests: 32, safe_commands: false }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StopReason {
    RequestBudget { limit: usize },
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestBudget { limit } => write!(f, "这一轮已经调用模型 {limit} 次，先停在这里（防止陷入循环）。回复「继续」可以接着做，已经做过的步骤 AI 能看到，不会从头再来"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_stops_at_the_request_ceiling() {
        let policy = ApiExecutionPolicy::default();
        assert_eq!(policy.max_requests, 32);
        assert!(policy.check_boundary(31).is_ok());
        assert_eq!(
            policy.check_boundary(32),
            Err(StopReason::RequestBudget { limit: 32 })
        );
    }
}
