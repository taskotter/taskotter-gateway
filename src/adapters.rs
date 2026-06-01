use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{api::GatewayError, policy::PolicyDecision};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Hosted,
    OpenAiCompatible,
    LocalRunner,
    FutureAdapter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRef {
    pub provider_id: String,
    pub kind: ProviderKind,
    pub model: String,
    #[serde(default)]
    pub endpoint_id: Option<String>,
    #[serde(default)]
    pub allowed_working_group_ids: Vec<String>,
    #[serde(default)]
    pub allowed_agent_ids: Vec<String>,
    #[serde(default)]
    pub allowed_workflow_ids: Vec<String>,
    #[serde(default)]
    pub allowed_models: Vec<String>,
    #[serde(default)]
    pub data_boundary_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedProviderRequest {
    pub request_id: Uuid,
    pub provider: ProviderRef,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub relay_id: Uuid,
    pub provider_kind: ProviderKind,
    pub model: String,
    pub output_text: String,
    pub stream_placeholder: bool,
}

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    async fn relay(
        &self,
        request: NormalizedProviderRequest,
        decision: PolicyDecision,
    ) -> Result<ProviderResponse, GatewayError>;
}

#[derive(Clone)]
pub struct ProviderRouter {
    stub: std::sync::Arc<dyn ProviderAdapter>,
}

impl ProviderRouter {
    pub fn new(stub: std::sync::Arc<dyn ProviderAdapter>) -> Self {
        Self { stub }
    }

    pub async fn route(
        &self,
        request: NormalizedProviderRequest,
        decision: PolicyDecision,
    ) -> Result<ProviderResponse, GatewayError> {
        self.stub.relay(request, decision).await
    }
}

pub struct StubProviderAdapter;

#[async_trait]
impl ProviderAdapter for StubProviderAdapter {
    async fn relay(
        &self,
        request: NormalizedProviderRequest,
        decision: PolicyDecision,
    ) -> Result<ProviderResponse, GatewayError> {
        if !decision.allowed {
            return Err(GatewayError::policy_denied(decision.reason));
        }

        if matches!(request.timeout_ms, Some(0)) {
            return Err(GatewayError::timeout(
                "provider relay timed out",
                request.timeout_ms,
            ));
        }

        if request
            .messages
            .iter()
            .any(|message| message.content.contains("simulate:mid_stream_quota"))
        {
            return Err(GatewayError::quota_exceeded(
                "usage quota exceeded during provider stream",
            ));
        }

        if let Some(max_tokens) = decision.max_tokens {
            let prompt_tokens = request
                .messages
                .iter()
                .map(|message| message.content.split_whitespace().count() as u64)
                .sum::<u64>();
            if prompt_tokens > max_tokens {
                return Err(GatewayError::quota_exceeded(
                    "provider request exceeds policy token quota",
                ));
            }
        }

        Ok(ProviderResponse {
            relay_id: Uuid::new_v4(),
            provider_kind: request.provider.kind,
            model: request.provider.model,
            output_text: "stub provider response".to_string(),
            stream_placeholder: request.stream,
        })
    }
}
