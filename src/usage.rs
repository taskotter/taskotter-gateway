use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{adapters::ProviderRef, policy::PolicySubject};

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
}
