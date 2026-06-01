use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::adapters::ProviderRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySubject {
    pub user_id: String,
    pub working_group_id: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub data_boundary_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCheck {
    pub subject: PolicySubject,
    pub provider: ProviderRef,
    pub operation: String,
    #[serde(default)]
    pub requested_max_tokens: Option<u64>,
    #[serde(default)]
    pub estimated_cost_micro_usd: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub decision_id: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_cost_micro_usd: Option<u64>,
}

#[async_trait]
pub trait PolicyEngine: Send + Sync {
    async fn evaluate(&self, check: PolicyCheck) -> PolicyDecision;
}

pub struct StaticPolicyEngine;

#[async_trait]
impl PolicyEngine for StaticPolicyEngine {
    async fn evaluate(&self, check: PolicyCheck) -> PolicyDecision {
        let reason = policy_denial_reason(&check);
        let denied = reason.is_some();

        PolicyDecision {
            allowed: !denied,
            decision_id: format!("local-policy:{}", check.operation),
            reason,
            max_tokens: Some(8_192),
            max_cost_micro_usd: Some(50_000),
        }
    }
}

fn policy_denial_reason(check: &PolicyCheck) -> Option<String> {
    if check.subject.user_id == "denied" || check.provider.provider_id.starts_with("disabled_") {
        return Some("request denied by gateway policy hook".to_string());
    }

    if !check.provider.allowed_working_group_ids.is_empty()
        && !check
            .provider
            .allowed_working_group_ids
            .contains(&check.subject.working_group_id)
    {
        return Some("provider not available to working group".to_string());
    }

    if !check.provider.allowed_agent_ids.is_empty()
        && check
            .subject
            .agent_id
            .as_ref()
            .is_none_or(|agent_id| !check.provider.allowed_agent_ids.contains(agent_id))
    {
        return Some("provider not bound to agent".to_string());
    }

    if !check.provider.allowed_workflow_ids.is_empty()
        && check
            .subject
            .workflow_id
            .as_ref()
            .is_none_or(|workflow_id| !check.provider.allowed_workflow_ids.contains(workflow_id))
    {
        return Some("provider not bound to workflow".to_string());
    }

    if !check.provider.allowed_models.is_empty()
        && !check
            .provider
            .allowed_models
            .contains(&check.provider.model)
    {
        return Some("model not allowed for provider binding".to_string());
    }

    if check.provider.data_boundary_id != check.subject.data_boundary_id {
        return Some("provider data boundary is not visible to subject".to_string());
    }

    if check
        .requested_max_tokens
        .is_some_and(|tokens| tokens > 8_192)
    {
        return Some("request exceeds token quota".to_string());
    }

    if check
        .estimated_cost_micro_usd
        .is_some_and(|cost| cost > 50_000)
    {
        return Some("request exceeds cost quota".to_string());
    }

    None
}
