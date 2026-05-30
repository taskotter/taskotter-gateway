//! Usage event contracts emitted by gateway operations.

use serde::{Deserialize, Serialize};

/// Principal responsible for usage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsagePrincipal {
    /// Principal kind.
    pub kind: PrincipalKind,
    /// Principal identifier.
    pub id: String,
    /// Working Group boundary for the principal.
    pub working_group_id: String,
}

/// Principal kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// A human user.
    User,
    /// An agent principal.
    Agent,
    /// A service principal.
    Service,
}

/// Usage subject.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsageSubject {
    /// Subject kind.
    pub kind: UsageSubjectKind,
}

/// Usage subject kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSubjectKind {
    /// Model call usage.
    ModelCall,
    /// Tool call usage.
    ToolCall,
    /// Automation run usage.
    AutomationRun,
    /// Runner job usage.
    RunnerJob,
    /// Gateway request usage.
    GatewayRequest,
}

/// Typed usage unit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsageUnit {
    /// Unit kind.
    pub kind: UsageUnitKind,
    /// Non-negative quantity.
    pub quantity: u64,
}

/// Usage unit kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageUnitKind {
    /// Input tokens.
    InputToken,
    /// Output tokens.
    OutputToken,
    /// Tool invocation count.
    ToolInvocation,
    /// Runtime milliseconds.
    RuntimeMillisecond,
    /// Stored bytes.
    StoredByte,
    /// Estimated cost in micro-USD.
    EstimatedCostMicrousd,
}

/// Usage event emitted before or after a gateway operation.
///
/// This shape intentionally mirrors
/// `https://schemas.taskotter.dev/v1/usage-event.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsageEvent {
    /// Stable event identifier supplied by the gateway.
    pub id: String,
    /// Working Group or tenant boundary.
    pub working_group_id: String,
    /// Principal responsible for the usage.
    pub principal: UsagePrincipal,
    /// Metered subject.
    pub subject: UsageSubject,
    /// Typed metered units.
    pub units: Vec<UsageUnit>,
    /// Retry-safe idempotency key.
    pub idempotency_key: String,
}

/// Gateway-local lifecycle stage used to derive usage idempotency keys.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageLifecycleStage {
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
