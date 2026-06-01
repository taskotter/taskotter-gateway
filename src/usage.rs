use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{adapters::ProviderRef, policy::PolicySubject};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingReasonCode {
    PrimarySelected,
    FallbackSelected,
    PolicyDenied,
    QuotaDenied,
    ProviderTimeout,
    ProviderError,
    Cancelled,
}

impl RoutingReasonCode {
    pub const METRIC_LABEL_VALUES: [&'static str; 7] = [
        "primary_selected",
        "fallback_selected",
        "policy_denied",
        "quota_denied",
        "provider_timeout",
        "provider_error",
        "cancelled",
    ];

    pub fn metric_label(self) -> &'static str {
        match self {
            Self::PrimarySelected => "primary_selected",
            Self::FallbackSelected => "fallback_selected",
            Self::PolicyDenied => "policy_denied",
            Self::QuotaDenied => "quota_denied",
            Self::ProviderTimeout => "provider_timeout",
            Self::ProviderError => "provider_error",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_metric_label(value: &str) -> bool {
        Self::METRIC_LABEL_VALUES.contains(&value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageAttemptStatus {
    Succeeded,
    Denied,
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
    pub routing_reason_code: RoutingReasonCode,
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
        let routing_reason_code = match status {
            UsageAttemptStatus::Succeeded => RoutingReasonCode::PrimarySelected,
            UsageAttemptStatus::Denied => RoutingReasonCode::PolicyDenied,
            UsageAttemptStatus::Timeout => RoutingReasonCode::ProviderTimeout,
            UsageAttemptStatus::Cancelled => RoutingReasonCode::Cancelled,
            UsageAttemptStatus::Failed => RoutingReasonCode::ProviderError,
        };

        Self::with_reason_code(
            request_id,
            subject,
            provider,
            decision_id,
            status,
            routing_reason_code,
        )
    }

    pub fn with_reason_code(
        request_id: Uuid,
        subject: PolicySubject,
        provider: ProviderRef,
        decision_id: String,
        status: UsageAttemptStatus,
        routing_reason_code: RoutingReasonCode,
    ) -> Self {
        Self {
            schema_version: "usage_audit_event.v1".to_string(),
            event_id: Uuid::new_v4(),
            request_id,
            correlation_id: None,
            subject,
            provider,
            decision_id,
            routing_reason_code,
            status,
            prompt_tokens: 0,
            completion_tokens: 0,
            estimated_cost_micro_usd: 0,
        }
    }
}
