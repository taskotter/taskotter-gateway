//! Usage event contracts emitted by gateway operations.

use serde::{Deserialize, Serialize};

/// Usage event emitted before or after a gateway operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsageEvent {
    /// Stable event identifier supplied by the gateway.
    pub event_id: String,
    /// Request identifier this event belongs to.
    pub request_id: String,
    /// Working Group or tenant boundary.
    pub working_group_id: String,
    /// Actor responsible for the usage.
    pub actor_id: String,
    /// Principal that performed the usage.
    pub principal_id: String,
    /// Capability identifier being metered.
    pub capability_id: String,
    /// Policy decision identifier that authorized or denied the action.
    pub policy_decision_id: String,
    /// Metered units. MVP treats this as a generic counter until specialized
    /// token, duration, request, and cost ledgers are connected.
    pub units: u64,
    /// Usage lifecycle stage.
    pub stage: UsageStage,
}

/// Usage lifecycle stage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageStage {
    /// A reservation or preflight estimate before execution.
    Reserved,
    /// Final usage after execution succeeds.
    Committed,
    /// Final usage after execution fails or is denied.
    Rejected,
}

/// Sink for usage events.
pub trait UsageMeter {
    /// Record a usage event.
    fn record(&self, event: &UsageEvent) -> Result<(), UsageMeterError>;
}

/// Usage meter write failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageMeterError {
    /// Redacted failure detail safe for logs and comments.
    pub message: String,
}

impl UsageMeterError {
    /// Creates a redacted usage meter error.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
