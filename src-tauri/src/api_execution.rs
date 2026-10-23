//! Explicit per-turn API execution budget: one turn may issue at most
//! `max_requests` model requests (tool rounds included), then stops.
use std::fmt;

#[derive(Clone, Debug)]
pub(crate) struct ApiExecutionPolicy {
    pub(crate) max_requests: usize,
}

impl Default for ApiExecutionPolicy {
    fn default() -> Self {
        Self { max_requests: 32 }
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
            Self::RequestBudget { limit } => write!(f, "本轮已达到 {limit} 次模型调用上限，已停止；请核对执行记录，避免重复操作"),
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
