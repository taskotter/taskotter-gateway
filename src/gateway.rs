//! Side-effect-light gateway planner.

use crate::policy::{PolicyCheck, PolicyDecision, PolicyEffect, PolicyEngine};
use crate::protocol::{GatewayRequest, ProtocolError};
use crate::tool::ToolCall;
use crate::usage::{
    PrincipalKind, UsageEvent, UsageLifecycleStage, UsageMeter, UsageMeterError, UsagePrincipal,
    UsageSubject, UsageSubjectKind, UsageUnit, UsageUnitKind,
};
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
        let lifecycle_stage = if decision.effect == PolicyEffect::Allow {
            UsageLifecycleStage::Reserved
        } else {
            UsageLifecycleStage::Rejected
        };
        let capability_id = request.payload.capability_id.clone();

        self.usage_meter.record(&usage_event(
            &request,
            capability_id,
            estimated_usage_units,
            lifecycle_stage,
        ))?;

        if decision.is_allowed() {
            Ok(GatewayOutcome::Allowed { decision })
        } else {
            Ok(GatewayOutcome::Denied { decision })
        }
    }
}

fn usage_event(
    request: &GatewayRequest<ToolCall>,
    capability_id: String,
    estimated_usage_units: u64,
    lifecycle_stage: UsageLifecycleStage,
) -> UsageEvent {
    let stage = match lifecycle_stage {
        UsageLifecycleStage::Reserved => "reserved",
        UsageLifecycleStage::Committed => "committed",
        UsageLifecycleStage::Rejected => "rejected",
    };

    UsageEvent {
        id: format!("usage_{}_{}", request.request_id, stage),
        working_group_id: request.working_group_id.clone(),
        principal: UsagePrincipal {
            kind: PrincipalKind::Agent,
            id: request.principal_id.clone(),
            working_group_id: request.working_group_id.clone(),
        },
        subject: UsageSubject {
            kind: UsageSubjectKind::GatewayRequest,
        },
        units: vec![UsageUnit {
            kind: UsageUnitKind::ToolInvocation,
            quantity: estimated_usage_units,
        }],
        idempotency_key: format!("gateway:{}:{}:{}", request.request_id, capability_id, stage),
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
    use crate::policy::{PolicyConstraint, PolicyDecision};
    use crate::tool::ToolCall;
    use crate::usage::{UsageSubjectKind, UsageUnitKind};
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
        let gateway = Gateway::new(StaticPolicy(PolicyDecision::allow()), usage);
        let request = GatewayRequest::new("req_1", "wg_1", "usr_1", "agent_1", tool_call());

        let outcome = gateway
            .plan_tool_call(request, 42)
            .expect("gateway outcome");

        assert_eq!(
            outcome,
            GatewayOutcome::Allowed {
                decision: PolicyDecision::allow()
            }
        );
        assert_eq!(
            gateway.usage_meter.events.borrow()[0].idempotency_key,
            "gateway:req_1:cap_mcp_filesystem:reserved"
        );
        assert_eq!(
            gateway.usage_meter.events.borrow()[0].units[0].kind,
            UsageUnitKind::ToolInvocation
        );
    }

    #[test]
    fn records_rejected_usage_when_policy_denies() {
        let usage = RecordingUsage::default();
        let gateway = Gateway::new(
            StaticPolicy(PolicyDecision::deny("usage_limit", "limit exceeded")),
            usage,
        );
        let request = GatewayRequest::new("req_2", "wg_1", "usr_1", "agent_1", tool_call());

        let outcome = gateway
            .plan_tool_call(request, 99)
            .expect("gateway outcome");

        assert_eq!(
            outcome,
            GatewayOutcome::Denied {
                decision: PolicyDecision::deny("usage_limit", "limit exceeded")
            }
        );
        assert_eq!(
            gateway.usage_meter.events.borrow()[0].idempotency_key,
            "gateway:req_2:cap_mcp_filesystem:rejected"
        );
    }

    #[test]
    fn serializes_policy_decision_like_control_plane_schema() {
        let decision = PolicyDecision {
            effect: PolicyEffect::Deny,
            constraints: vec![
                PolicyConstraint::allow("working_group_access", "allowed"),
                PolicyConstraint::deny("missing_secret", "secret unavailable"),
            ],
        };

        assert_eq!(
            serde_json::to_value(decision).expect("policy json"),
            json!({
                "effect": "deny",
                "constraints": [
                    {
                        "name": "working_group_access",
                        "effect": "allow",
                        "reason": "allowed"
                    },
                    {
                        "name": "missing_secret",
                        "effect": "deny",
                        "reason": "secret unavailable"
                    }
                ]
            })
        );
    }

    #[test]
    fn serializes_common_policy_denial_fixtures() {
        let fixture_names = ["quota_exceeded", "missing_secret", "unknown_capability"];

        for name in fixture_names {
            let decision = PolicyDecision::deny(name, "denied by test fixture");
            let value = serde_json::to_value(decision).expect("policy fixture json");

            assert_eq!(value["effect"], "deny");
            assert_eq!(value["constraints"][0]["name"], name);
            assert_eq!(value["constraints"][0]["effect"], "deny");
        }
    }

    #[test]
    fn serializes_usage_event_like_control_plane_schema() {
        let usage = RecordingUsage::default();
        let gateway = Gateway::new(StaticPolicy(PolicyDecision::allow()), usage);
        let request = GatewayRequest::new("req_3", "wg_1", "usr_1", "agent_1", tool_call());

        gateway.plan_tool_call(request, 1).expect("gateway outcome");
        let event = gateway.usage_meter.events.borrow()[0].clone();

        assert_eq!(event.subject.kind, UsageSubjectKind::GatewayRequest);
        assert_eq!(
            serde_json::to_value(event).expect("usage event json"),
            json!({
                "id": "usage_req_3_reserved",
                "working_group_id": "wg_1",
                "principal": {
                    "kind": "agent",
                    "id": "agent_1",
                    "working_group_id": "wg_1"
                },
                "subject": {
                    "kind": "gateway_request"
                },
                "units": [
                    {
                        "kind": "tool_invocation",
                        "quantity": 1
                    }
                ],
                "idempotency_key": "gateway:req_3:cap_mcp_filesystem:reserved"
            })
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
