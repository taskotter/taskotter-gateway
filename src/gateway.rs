//! Side-effect-light gateway planner.

use crate::policy::{PolicyCheck, PolicyDecision, PolicyEffect, PolicyEngine};
use crate::protocol::{GatewayRequest, ProtocolError};
use crate::tool::ToolCall;
use crate::usage::{UsageEvent, UsageMeter, UsageMeterError, UsageStage};
use std::fmt;

/// Gateway planner that validates protocol, asks policy, and emits usage.
pub struct Gateway<P, U> {
    policy_engine: P,
    usage_meter: U,
}

impl<P, U> Gateway<P, U>
where
    P: PolicyEngine,
    U: UsageMeter,
{
    /// Creates a gateway planner with supplied integration adapters.
    #[must_use]
    pub fn new(policy_engine: P, usage_meter: U) -> Self {
        Self {
            policy_engine,
            usage_meter,
        }
    }

    /// Plans a tool call and returns the auditable decision.
    pub fn plan_tool_call(
        &self,
        request: GatewayRequest<ToolCall>,
        estimated_usage_units: u64,
    ) -> Result<GatewayOutcome, GatewayError> {
        request.validate()?;

        let check = PolicyCheck {
            working_group_id: request.working_group_id.clone(),
            actor_id: request.actor_id.clone(),
            principal_id: request.principal_id.clone(),
            tool_call: request.payload.clone(),
            estimated_usage_units,
        };
        let decision = self.policy_engine.evaluate(&check);
        let stage = if decision.effect == PolicyEffect::Allow {
            UsageStage::Reserved
        } else {
            UsageStage::Rejected
        };

        self.usage_meter.record(&UsageEvent {
            event_id: format!("usage_{}", request.request_id),
            request_id: request.request_id,
            working_group_id: request.working_group_id,
            actor_id: request.actor_id,
            principal_id: request.principal_id,
            capability_id: request.payload.capability_id,
            policy_decision_id: decision.decision_id.clone(),
            units: estimated_usage_units,
            stage,
        })?;

        if decision.is_allowed() {
            Ok(GatewayOutcome::Allowed { decision })
        } else {
            Ok(GatewayOutcome::Denied { decision })
        }
    }
}

/// Gateway planning outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GatewayOutcome {
    /// Policy allowed the gateway to dispatch the call.
    Allowed {
        /// Auditable policy decision.
        decision: PolicyDecision,
    },
    /// Policy denied dispatch.
    Denied {
        /// Auditable policy decision.
        decision: PolicyDecision,
    },
}

/// Gateway planning failure.
#[derive(Debug)]
pub enum GatewayError {
    /// The request failed protocol validation.
    Protocol(ProtocolError),
    /// The usage meter could not record the decision.
    Usage(UsageMeterError),
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(f, "{error}"),
            Self::Usage(error) => write!(f, "usage meter failed: {}", error.message),
        }
    }
}

impl std::error::Error for GatewayError {}

impl From<ProtocolError> for GatewayError {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

impl From<UsageMeterError> for GatewayError {
    fn from(error: UsageMeterError) -> Self {
        Self::Usage(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::PolicyDecision;
    use crate::tool::ToolCall;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::cell::RefCell;

    struct StaticPolicy(PolicyDecision);

    impl PolicyEngine for StaticPolicy {
        fn evaluate(&self, _check: &PolicyCheck) -> PolicyDecision {
            self.0.clone()
        }
    }

    #[derive(Default)]
    struct RecordingUsage {
        events: RefCell<Vec<UsageEvent>>,
    }

    impl UsageMeter for RecordingUsage {
        fn record(&self, event: &UsageEvent) -> Result<(), UsageMeterError> {
            self.events.borrow_mut().push(event.clone());
            Ok(())
        }
    }

    #[test]
    fn records_reserved_usage_when_policy_allows() {
        let usage = RecordingUsage::default();
        let gateway = Gateway::new(StaticPolicy(PolicyDecision::allow("decision_1")), usage);
        let request = GatewayRequest::new("req_1", "wg_1", "usr_1", "agent_1", tool_call());

        let outcome = gateway
            .plan_tool_call(request, 42)
            .expect("gateway outcome");

        assert_eq!(
            outcome,
            GatewayOutcome::Allowed {
                decision: PolicyDecision::allow("decision_1")
            }
        );
        assert_eq!(
            gateway.usage_meter.events.borrow()[0].stage,
            UsageStage::Reserved
        );
    }

    #[test]
    fn records_rejected_usage_when_policy_denies() {
        let usage = RecordingUsage::default();
        let gateway = Gateway::new(
            StaticPolicy(PolicyDecision::deny("decision_2", "limit exceeded")),
            usage,
        );
        let request = GatewayRequest::new("req_2", "wg_1", "usr_1", "agent_1", tool_call());

        let outcome = gateway
            .plan_tool_call(request, 99)
            .expect("gateway outcome");

        assert_eq!(
            outcome,
            GatewayOutcome::Denied {
                decision: PolicyDecision::deny("decision_2", "limit exceeded")
            }
        );
        assert_eq!(
            gateway.usage_meter.events.borrow()[0].stage,
            UsageStage::Rejected
        );
    }

    fn tool_call() -> ToolCall {
        ToolCall {
            capability_id: "cap_mcp_filesystem".to_owned(),
            operation: "tools.call".to_owned(),
            arguments: json!({ "path": "README.md" }),
        }
    }
}
