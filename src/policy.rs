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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCheck {
    pub subject: PolicySubject,
    pub provider: ProviderRef,
    pub operation: String,
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
        let denied = check.subject.user_id == "denied"
            || check.provider.provider_id.starts_with("disabled_");

        PolicyDecision {
            allowed: !denied,
            decision_id: format!("local-policy:{}", check.operation),
            reason: denied.then_some("request denied by gateway policy hook".to_string()),
            max_tokens: Some(8_192),
            max_cost_micro_usd: Some(50_000),
        }
    }
}
