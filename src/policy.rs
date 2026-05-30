//! Policy decision boundary for gateway requests.

use crate::tool::ToolCall;
use serde::{Deserialize, Serialize};

/// A policy check assembled before a gateway operation can proceed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyCheck {
    /// Working Group or tenant boundary.
    pub working_group_id: String,
    /// User initiating the request.
    pub actor_id: String,
    /// Agent, workflow, or runtime principal acting on behalf of the user.
    pub principal_id: String,
    /// Tool call under evaluation.
    pub tool_call: ToolCall,
    /// Estimated maximum usage units before execution.
    pub estimated_usage_units: u64,
}

/// Allow or deny result from the policy engine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEffect {
    /// The request can proceed.
    Allow,
    /// The request must not proceed.
    Deny,
}

/// Auditable policy decision returned by the control plane.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// Decision effect.
    pub effect: PolicyEffect,
    /// Stable policy decision identifier for audit correlation.
    pub decision_id: String,
    /// Human-readable denial or audit reason.
    pub reason: String,
}

impl PolicyDecision {
    /// Builds an allow decision.
    #[must_use]
    pub fn allow(decision_id: impl Into<String>) -> Self {
        Self {
            effect: PolicyEffect::Allow,
            decision_id: decision_id.into(),
            reason: "allowed".to_owned(),
        }
    }

    /// Builds a deny decision.
    #[must_use]
    pub fn deny(decision_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            effect: PolicyEffect::Deny,
            decision_id: decision_id.into(),
            reason: reason.into(),
        }
    }

    /// Returns true when the decision allows execution.
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        self.effect == PolicyEffect::Allow
    }
}

/// Gateway-facing policy engine abstraction.
pub trait PolicyEngine {
    /// Evaluate a policy check before the gateway dispatches work.
    fn evaluate(&self, check: &PolicyCheck) -> PolicyDecision;
}
