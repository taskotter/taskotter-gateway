use serde::{Deserialize, Serialize};

use crate::{
    adapters::ProviderKind,
    contracts::{
        ActorRef, AuditEvent, AuditOutcome, AuditPayload, EventActorRef, EventResourceRef,
        EventSource, NormalizedError, NormalizedErrorCode, UsageEvent, UsageMeasurements,
        UsagePayload, UsageSubject, UsageSubjectType,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackAttemptKind {
    Retry,
    SameProvider,
    CrossProvider,
    RunnerLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingReasonCode {
    PrimaryRateLimited,
    PrimaryTimeout,
    PrimaryUnavailable,
    PrimaryMalformedResponse,
    RunnerLocalRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredProviderCapability {
    Streaming,
    JsonOutput,
    ToolCalls,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackPolicyLimits {
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_cost_micro_usd: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackProviderCandidate {
    pub provider_id: String,
    pub provider_kind: ProviderKind,
    pub provider_family: String,
    pub model: String,
    pub region: String,
    pub data_residency: String,
    pub credential_scope: String,
    #[serde(default)]
    pub supports_streaming: bool,
    #[serde(default)]
    pub supports_json_output: bool,
    #[serde(default)]
    pub supports_tool_calls: bool,
}

impl FallbackProviderCandidate {
    fn supports(&self, capability: RequiredProviderCapability) -> bool {
        match capability {
            RequiredProviderCapability::Streaming => self.supports_streaming,
            RequiredProviderCapability::JsonOutput => self.supports_json_output,
            RequiredProviderCapability::ToolCalls => self.supports_tool_calls,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackPolicyRequest {
    pub request_id: String,
    pub correlation_id: String,
    pub working_group_id: String,
    pub actor: ActorRef,
    pub policy_decision_id: String,
    pub attempt_kind: FallbackAttemptKind,
    pub routing_reason: RoutingReasonCode,
    pub primary: FallbackProviderCandidate,
    pub candidate: FallbackProviderCandidate,
    #[serde(default)]
    pub required_capabilities: Vec<RequiredProviderCapability>,
    #[serde(default)]
    pub original_limits: FallbackPolicyLimits,
    #[serde(default)]
    pub candidate_limits: FallbackPolicyLimits,
    #[serde(default)]
    pub allow_cross_provider: bool,
    #[serde(default)]
    pub allow_runner_local: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackPolicyDecision {
    pub allowed: bool,
    pub reason: RoutingReasonCode,
    pub selected_provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackPolicyFailure {
    pub class: &'static str,
    pub message: String,
    pub normalized_error: NormalizedError,
    pub usage_event: UsageEvent,
    pub audit_event: AuditEvent,
}

pub fn validate_fallback(
    request: &FallbackPolicyRequest,
) -> Result<FallbackPolicyDecision, Box<FallbackPolicyFailure>> {
    if !is_retryable_reason(request.routing_reason) {
        return Err(failure(
            request,
            "fallback_reason_not_retryable",
            "Fallback is only allowed for retryable routing reasons.",
        ));
    }

    validate_attempt_boundary(request)?;
    validate_required_capabilities(request)?;
    validate_scope_boundary(request)?;
    validate_policy_limits(request)?;

    Ok(FallbackPolicyDecision {
        allowed: true,
        reason: request.routing_reason,
        selected_provider: request.candidate.provider_id.clone(),
    })
}

fn validate_attempt_boundary(
    request: &FallbackPolicyRequest,
) -> Result<(), Box<FallbackPolicyFailure>> {
    if request.candidate.provider_kind == ProviderKind::LocalRunner
        && !(request.attempt_kind == FallbackAttemptKind::RunnerLocal && request.allow_runner_local)
    {
        return Err(failure(
            request,
            "runner_local_fallback_not_allowed",
            "Runner-local fallback requires an explicit runner-local attempt and policy allowance.",
        ));
    }

    match request.attempt_kind {
        FallbackAttemptKind::Retry => {
            if request.primary.provider_id == request.candidate.provider_id
                && request.primary.model == request.candidate.model
            {
                Ok(())
            } else {
                Err(failure(
                    request,
                    "retry_target_changed",
                    "Retry fallback must keep the same provider and model.",
                ))
            }
        }
        FallbackAttemptKind::SameProvider => {
            if request.primary.provider_family == request.candidate.provider_family {
                Ok(())
            } else {
                Err(failure(
                    request,
                    "same_provider_family_changed",
                    "Same-provider fallback cannot change provider family.",
                ))
            }
        }
        FallbackAttemptKind::CrossProvider => {
            if request.allow_cross_provider {
                Ok(())
            } else {
                Err(failure(
                    request,
                    "cross_provider_fallback_not_allowed",
                    "Cross-provider fallback requires an explicit policy allowance.",
                ))
            }
        }
        FallbackAttemptKind::RunnerLocal => {
            if request.allow_runner_local
                && request.candidate.provider_kind == ProviderKind::LocalRunner
            {
                Ok(())
            } else {
                Err(failure(
                    request,
                    "runner_local_fallback_not_allowed",
                    "Runner-local fallback requires an explicit policy allowance and local runner candidate.",
                ))
            }
        }
    }
}

fn validate_required_capabilities(
    request: &FallbackPolicyRequest,
) -> Result<(), Box<FallbackPolicyFailure>> {
    for capability in &request.required_capabilities {
        if !request.candidate.supports(*capability) {
            return Err(failure(
                request,
                "required_capability_dropped",
                "Fallback candidate does not satisfy a required provider capability.",
            ));
        }
    }
    Ok(())
}

fn validate_scope_boundary(
    request: &FallbackPolicyRequest,
) -> Result<(), Box<FallbackPolicyFailure>> {
    let scope_matches = request.primary.region == request.candidate.region
        && request.primary.data_residency == request.candidate.data_residency
        && request.primary.credential_scope == request.candidate.credential_scope;

    if scope_matches {
        Ok(())
    } else {
        Err(failure(
            request,
            "fallback_scope_widened",
            "Fallback cannot widen region, data residency, or credential scope.",
        ))
    }
}

fn validate_policy_limits(
    request: &FallbackPolicyRequest,
) -> Result<(), Box<FallbackPolicyFailure>> {
    if limit_widened(
        request.original_limits.max_tokens,
        request.candidate_limits.max_tokens,
    ) || limit_widened(
        request.original_limits.max_cost_micro_usd,
        request.candidate_limits.max_cost_micro_usd,
    ) {
        return Err(failure(
            request,
            "fallback_policy_limit_widened",
            "Fallback cannot raise token or cost limits from the original policy decision.",
        ));
    }
    Ok(())
}

fn limit_widened(original: Option<u64>, candidate: Option<u64>) -> bool {
    match (original, candidate) {
        (Some(original), Some(candidate)) => candidate > original,
        (Some(_), None) => true,
        _ => false,
    }
}

fn is_retryable_reason(reason: RoutingReasonCode) -> bool {
    matches!(
        reason,
        RoutingReasonCode::PrimaryRateLimited
            | RoutingReasonCode::PrimaryTimeout
            | RoutingReasonCode::PrimaryUnavailable
            | RoutingReasonCode::PrimaryMalformedResponse
            | RoutingReasonCode::RunnerLocalRequired
    )
}

fn failure(
    request: &FallbackPolicyRequest,
    class: &'static str,
    message: &'static str,
) -> Box<FallbackPolicyFailure> {
    Box::new(FallbackPolicyFailure {
        class,
        message: message.to_string(),
        normalized_error: NormalizedError {
            code: NormalizedErrorCode::PolicyDenied,
            message: message.to_string(),
            retryable: false,
            upstream_status: None,
            provider_error_class: Some(class.to_string()),
        },
        usage_event: usage_event(request, class),
        audit_event: audit_event(request, class),
    })
}

fn usage_event(request: &FallbackPolicyRequest, class: &str) -> UsageEvent {
    UsageEvent {
        id: format!("evt_{}_fallback_denied_usage", request.request_id),
        event_type: "usage.gateway_request.recorded".to_string(),
        version: "0.1.0".to_string(),
        occurred_at: "2026-01-01T00:00:01.000Z".to_string(),
        source: EventSource::Gateway,
        working_group_id: request.working_group_id.clone(),
        actor: EventActorRef::from(&request.actor),
        resource: EventResourceRef {
            resource_type: "provider".to_string(),
            id: request.candidate.provider_id.clone(),
        },
        correlation_id: request.correlation_id.clone(),
        request_id: request.request_id.clone(),
        policy_decision_id: request.policy_decision_id.clone(),
        idempotency_key: format!("usage_{}_fallback_denied", request.request_id),
        payload: UsagePayload {
            subject: UsageSubject {
                subject_type: UsageSubjectType::GatewayRequest,
                id: request.request_id.clone(),
            },
            measurements: UsageMeasurements {
                duration_ms: 0,
                input_tokens: Some(0),
                output_tokens: Some(0),
                tool_invocations: Some(0),
                estimated_cost_micros: Some(0),
                metering_unit: None,
                runtime_capability: Some(format!("gateway.fallback_denied.{class}")),
            },
            routing: None,
        },
    }
}

fn audit_event(request: &FallbackPolicyRequest, class: &str) -> AuditEvent {
    AuditEvent {
        id: format!("evt_{}_fallback_denied_audit", request.request_id),
        event_type: "audit.policy_decision.denied".to_string(),
        version: "0.1.0".to_string(),
        occurred_at: "2026-01-01T00:00:02.000Z".to_string(),
        source: EventSource::Gateway,
        working_group_id: request.working_group_id.clone(),
        actor: EventActorRef::from(&request.actor),
        resource: EventResourceRef {
            resource_type: "provider".to_string(),
            id: request.candidate.provider_id.clone(),
        },
        correlation_id: request.correlation_id.clone(),
        request_id: request.request_id.clone(),
        policy_decision_id: request.policy_decision_id.clone(),
        payload: AuditPayload {
            action: "gateway.provider.fallback".to_string(),
            outcome: AuditOutcome::Denied,
            routing: None,
            runtime_capability: Some(format!("gateway.fallback_denied.{class}")),
            feature_flag: Some("gateway.provider_routing.enabled".to_string()),
            approval_ref: Some(request.policy_decision_id.clone()),
        },
    }
}
