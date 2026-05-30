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

/// Allow or deny result used by policy decisions and constraints.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEffect {
    /// The request can proceed.
    Allow,
    /// The request must not proceed.
    Deny,
}

/// One named policy constraint contributing to the final decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyConstraint {
    /// Stable constraint name, such as `usage_limit` or `integration_access`.
    pub name: String,
    /// Constraint effect.
    pub effect: PolicyEffect,
    /// Human-readable reason safe for audit records.
    pub reason: String,
}

impl PolicyConstraint {
    /// Builds an allow constraint.
    #[must_use]
    pub fn allow(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            effect: PolicyEffect::Allow,
            reason: reason.into(),
        }
    }

    /// Builds a deny constraint.
    #[must_use]
    pub fn deny(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            effect: PolicyEffect::Deny,
            reason: reason.into(),
        }
    }
}

/// Canonical policy decision returned by the control plane.
///
/// This shape intentionally mirrors
/// `https://schemas.taskotter.dev/v1/policy-decision.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// Decision effect.
    pub effect: PolicyEffect,
    /// Individual constraints that were evaluated with AND semantics.
    pub constraints: Vec<PolicyConstraint>,
}

impl PolicyDecision {
    /// Builds an allow decision.
    #[must_use]
    pub fn allow() -> Self {
        Self {
            effect: PolicyEffect::Allow,
            constraints: vec![PolicyConstraint::allow("gateway_preflight", "allowed")],
        }
    }

    /// Builds a deny decision.
    #[must_use]
    pub fn deny(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            effect: PolicyEffect::Deny,
            constraints: vec![PolicyConstraint::deny(name, reason)],
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
