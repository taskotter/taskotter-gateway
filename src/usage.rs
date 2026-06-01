use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    adapters::ProviderRef,
    contracts::{
        AuditEvent, AuditOutcome, AuditPayload, EventActorRef, EventActorType, EventResourceRef,
        EventSource, UsageEvent, UsageMeasurements, UsagePayload, UsageSubject, UsageSubjectType,
    },
    policy::PolicySubject,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageAttemptStatus {
    Succeeded,
    Denied,
    QuotaDenied,
    Timeout,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageAuditEventV1 {
    pub schema_version: String,
    pub event_id: Uuid,
    pub request_id: Uuid,
    #[serde(default)]
    pub correlation_id: Option<String>,
    pub subject: PolicySubject,
    pub provider: ProviderRef,
    pub decision_id: String,
    pub idempotency_key: String,
    pub status: UsageAttemptStatus,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub estimated_cost_micro_usd: u64,
}

impl UsageAuditEventV1 {
    pub fn new(
        request_id: Uuid,
        subject: PolicySubject,
        provider: ProviderRef,
        decision_id: String,
        status: UsageAttemptStatus,
    ) -> Self {
        Self {
            schema_version: "usage_audit_event.v1".to_string(),
            event_id: Uuid::new_v4(),
            request_id,
            correlation_id: None,
            subject,
            provider,
            decision_id,
            idempotency_key: format!("usage:{request_id}"),
            status,
            prompt_tokens: 0,
            completion_tokens: 0,
            estimated_cost_micro_usd: 0,
        }
    }

    pub fn to_usage_event(&self) -> UsageEvent {
        UsageEvent {
            id: self.event_id.to_string(),
            event_type: "usage.gateway_request.recorded".to_string(),
            version: "0.1.0".to_string(),
            occurred_at: "2026-01-01T00:00:00.000Z".to_string(),
            source: EventSource::Gateway,
            working_group_id: self.subject.working_group_id.clone(),
            actor: event_actor(&self.subject),
            resource: EventResourceRef {
                resource_type: "provider".to_string(),
                id: self.provider.provider_id.clone(),
            },
            correlation_id: self.correlation_id.clone().unwrap_or_default(),
            request_id: self.request_id.to_string(),
            policy_decision_id: self.decision_id.clone(),
            idempotency_key: self.idempotency_key.clone(),
            payload: UsagePayload {
                subject: UsageSubject {
                    subject_type: UsageSubjectType::GatewayRequest,
                    id: self.request_id.to_string(),
                },
                measurements: UsageMeasurements {
                    duration_ms: 0,
                    input_tokens: Some(self.prompt_tokens.min(u32::MAX as u64) as u32),
                    output_tokens: Some(self.completion_tokens.min(u32::MAX as u64) as u32),
                    tool_invocations: Some(0),
                    estimated_cost_micros: Some(self.estimated_cost_micro_usd),
                    metering_unit: None,
                    runtime_capability: Some("gateway.provider_routing".to_string()),
                },
            },
        }
    }

    pub fn to_audit_event(&self) -> AuditEvent {
        AuditEvent {
            id: self.event_id.to_string(),
            event_type: format!("audit.gateway_request.{}", self.status.contract_name()),
            version: "0.1.0".to_string(),
            occurred_at: "2026-01-01T00:00:00.000Z".to_string(),
            source: EventSource::Gateway,
            working_group_id: self.subject.working_group_id.clone(),
            actor: event_actor(&self.subject),
            resource: EventResourceRef {
                resource_type: "provider".to_string(),
                id: self.provider.provider_id.clone(),
            },
            correlation_id: self.correlation_id.clone().unwrap_or_default(),
            request_id: self.request_id.to_string(),
            policy_decision_id: self.decision_id.clone(),
            payload: AuditPayload {
                action: "gateway.provider.invoke".to_string(),
                outcome: self.status.audit_outcome(),
                runtime_capability: Some("gateway.provider_routing".to_string()),
                feature_flag: Some("gateway.provider_routing.enabled".to_string()),
                approval_ref: None,
            },
        }
    }
}

impl UsageAttemptStatus {
    fn contract_name(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Denied => "denied",
            Self::QuotaDenied => "quota_denied",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    fn audit_outcome(&self) -> AuditOutcome {
        match self {
            Self::Succeeded => AuditOutcome::Succeeded,
            Self::Denied | Self::QuotaDenied => AuditOutcome::Denied,
            Self::Timeout | Self::Cancelled | Self::Failed => AuditOutcome::Failed,
        }
    }
}

fn event_actor(subject: &PolicySubject) -> EventActorRef {
    if let Some(agent_id) = &subject.agent_id {
        return EventActorRef {
            actor_type: EventActorType::Agent,
            id: agent_id.clone(),
        };
    }

    EventActorRef {
        actor_type: EventActorType::User,
        id: subject.user_id.clone(),
    }
}
