use crate::domain::StageStatus;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeTransitionReason {
    RuntimeClaim,
    RuntimeStart,
    RuntimeSuccess,
    RuntimeRetryScheduled,
    RuntimeFailed,
    RuntimeBlocked,
    StuckReconciliation,
    ManualRetryNow,
    ManualReset,
    ManualSkip,
    ClaimRecovery,
}

impl RuntimeTransitionReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeClaim => "runtime_claim",
            Self::RuntimeStart => "runtime_start",
            Self::RuntimeSuccess => "runtime_success",
            Self::RuntimeRetryScheduled => "runtime_retry_scheduled",
            Self::RuntimeFailed => "runtime_failed",
            Self::RuntimeBlocked => "runtime_blocked",
            Self::StuckReconciliation => "stuck_reconciliation",
            Self::ManualRetryNow => "manual_retry_now",
            Self::ManualReset => "manual_reset",
            Self::ManualSkip => "manual_skip",
            Self::ClaimRecovery => "claim_recovery",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeTransitionError {
    pub from_status: String,
    pub to_status: String,
    pub reason: String,
    pub state_id: Option<i64>,
    pub entity_id: Option<String>,
    pub stage_id: Option<String>,
    pub message: String,
}

impl std::fmt::Display for RuntimeTransitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

pub(crate) fn validate_transition(
    from: &StageStatus,
    to: &StageStatus,
    reason: RuntimeTransitionReason,
) -> Result<(), RuntimeTransitionError> {
    let allowed = matches!(
        (reason, from, to),
        (
            RuntimeTransitionReason::RuntimeClaim,
            StageStatus::Pending,
            StageStatus::Queued
        ) | (
            RuntimeTransitionReason::RuntimeClaim,
            StageStatus::RetryWait,
            StageStatus::Queued
        ) | (
            RuntimeTransitionReason::RuntimeStart,
            StageStatus::Queued,
            StageStatus::InProgress
        ) | (
            RuntimeTransitionReason::RuntimeSuccess,
            StageStatus::InProgress,
            StageStatus::Done
        ) | (
            RuntimeTransitionReason::RuntimeRetryScheduled,
            StageStatus::InProgress,
            StageStatus::RetryWait
        ) | (
            RuntimeTransitionReason::RuntimeFailed,
            StageStatus::InProgress,
            StageStatus::Failed
        ) | (
            RuntimeTransitionReason::RuntimeFailed,
            StageStatus::RetryWait,
            StageStatus::Failed
        ) | (
            RuntimeTransitionReason::RuntimeBlocked,
            StageStatus::Pending,
            StageStatus::Blocked
        ) | (
            RuntimeTransitionReason::RuntimeBlocked,
            StageStatus::Queued,
            StageStatus::Blocked
        ) | (
            RuntimeTransitionReason::RuntimeBlocked,
            StageStatus::InProgress,
            StageStatus::Blocked
        ) | (
            RuntimeTransitionReason::StuckReconciliation,
            StageStatus::InProgress,
            StageStatus::RetryWait
        ) | (
            RuntimeTransitionReason::StuckReconciliation,
            StageStatus::InProgress,
            StageStatus::Failed
        ) | (
            RuntimeTransitionReason::ClaimRecovery,
            StageStatus::Queued,
            StageStatus::Pending
        ) | (
            RuntimeTransitionReason::ManualReset,
            StageStatus::Failed,
            StageStatus::Pending
        ) | (
            RuntimeTransitionReason::ManualReset,
            StageStatus::Blocked,
            StageStatus::Pending
        ) | (
            RuntimeTransitionReason::ManualReset,
            StageStatus::RetryWait,
            StageStatus::Pending
        ) | (
            RuntimeTransitionReason::ManualReset,
            StageStatus::Skipped,
            StageStatus::Pending
        ) | (
            RuntimeTransitionReason::ManualSkip,
            StageStatus::Pending,
            StageStatus::Skipped
        ) | (
            RuntimeTransitionReason::ManualSkip,
            StageStatus::RetryWait,
            StageStatus::Skipped
        )
    );

    if allowed {
        Ok(())
    } else {
        Err(RuntimeTransitionError {
            from_status: status_value(from).to_string(),
            to_status: status_value(to).to_string(),
            reason: reason.as_str().to_string(),
            state_id: None,
            entity_id: None,
            stage_id: None,
            message: format!(
                "Invalid runtime transition from '{}' to '{}' for reason '{}'.",
                status_value(from),
                status_value(to),
                reason.as_str()
            ),
        })
    }
}

pub(crate) fn parse_status(value: &str) -> Option<StageStatus> {
    match value {
        "pending" => Some(StageStatus::Pending),
        "queued" => Some(StageStatus::Queued),
        "in_progress" => Some(StageStatus::InProgress),
        "retry_wait" => Some(StageStatus::RetryWait),
        "done" => Some(StageStatus::Done),
        "failed" => Some(StageStatus::Failed),
        "blocked" => Some(StageStatus::Blocked),
        "skipped" => Some(StageStatus::Skipped),
        _ => None,
    }
}

pub(crate) fn status_value(status: &StageStatus) -> &'static str {
    match status {
        StageStatus::Pending => "pending",
        StageStatus::Queued => "queued",
        StageStatus::InProgress => "in_progress",
        StageStatus::RetryWait => "retry_wait",
        StageStatus::Done => "done",
        StageStatus::Failed => "failed",
        StageStatus::Blocked => "blocked",
        StageStatus::Skipped => "skipped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_required_runtime_transitions() {
        let cases = [
            (
                StageStatus::Pending,
                StageStatus::Queued,
                RuntimeTransitionReason::RuntimeClaim,
            ),
            (
                StageStatus::RetryWait,
                StageStatus::Queued,
                RuntimeTransitionReason::RuntimeClaim,
            ),
            (
                StageStatus::Queued,
                StageStatus::InProgress,
                RuntimeTransitionReason::RuntimeStart,
            ),
            (
                StageStatus::InProgress,
                StageStatus::Done,
                RuntimeTransitionReason::RuntimeSuccess,
            ),
            (
                StageStatus::InProgress,
                StageStatus::RetryWait,
                RuntimeTransitionReason::RuntimeRetryScheduled,
            ),
            (
                StageStatus::InProgress,
                StageStatus::Failed,
                RuntimeTransitionReason::RuntimeFailed,
            ),
            (
                StageStatus::InProgress,
                StageStatus::Blocked,
                RuntimeTransitionReason::RuntimeBlocked,
            ),
            (
                StageStatus::Queued,
                StageStatus::Pending,
                RuntimeTransitionReason::ClaimRecovery,
            ),
            (
                StageStatus::Failed,
                StageStatus::Pending,
                RuntimeTransitionReason::ManualReset,
            ),
            (
                StageStatus::Blocked,
                StageStatus::Pending,
                RuntimeTransitionReason::ManualReset,
            ),
            (
                StageStatus::RetryWait,
                StageStatus::Pending,
                RuntimeTransitionReason::ManualReset,
            ),
            (
                StageStatus::Pending,
                StageStatus::Skipped,
                RuntimeTransitionReason::ManualSkip,
            ),
            (
                StageStatus::RetryWait,
                StageStatus::Skipped,
                RuntimeTransitionReason::ManualSkip,
            ),
        ];

        for (from, to, reason) in cases {
            validate_transition(&from, &to, reason).expect("transition should be allowed");
        }
    }

    #[test]
    fn rejects_invalid_runtime_transitions() {
        let cases = [
            (
                StageStatus::Pending,
                StageStatus::Done,
                RuntimeTransitionReason::RuntimeSuccess,
            ),
            (
                StageStatus::Done,
                StageStatus::InProgress,
                RuntimeTransitionReason::RuntimeStart,
            ),
            (
                StageStatus::Failed,
                StageStatus::Queued,
                RuntimeTransitionReason::RuntimeClaim,
            ),
            (
                StageStatus::Blocked,
                StageStatus::InProgress,
                RuntimeTransitionReason::RuntimeStart,
            ),
        ];

        for (from, to, reason) in cases {
            let error =
                validate_transition(&from, &to, reason).expect_err("transition should reject");
            assert_eq!(error.from_status, status_value(&from));
            assert_eq!(error.to_status, status_value(&to));
        }
    }
}
