use crate::contracts::{
    EventActorRef, EventResourceRef, EventSource, FinishReason, ModelCapability, ModelResponse,
    NormalizedError, NormalizedErrorCode, ProviderAdapterCapability, ProviderCapabilityKind,
    ProviderRoutingMetadata, ScopedModelRequest, StreamFrame, StreamFrameType, UsageEvent,
    UsageMeasurement, UsageMeasurements, UsagePayload, UsageSubject, UsageSubjectType,
};

#[derive(Debug, thiserror::Error)]
pub enum ProviderAdapterError {
    #[error("gateway request was rejected: {0}")]
    Rejected(String),
}

pub trait ProviderAdapter {
    fn capability(&self) -> ProviderAdapterCapability;
    fn complete(&self, request: &ScopedModelRequest) -> Result<ModelResponse, NormalizedError>;
    fn stream(&self, request: &ScopedModelRequest) -> Result<Vec<StreamFrame>, NormalizedError>;
    fn usage_event(
        &self,
        request: &ScopedModelRequest,
        response: Option<&ModelResponse>,
        error: Option<&NormalizedError>,
    ) -> UsageEvent;
}

#[derive(Debug, Clone, Default)]
pub struct FakeProviderAdapter;

impl FakeProviderAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl ProviderAdapter for FakeProviderAdapter {
    fn capability(&self) -> ProviderAdapterCapability {
        ProviderAdapterCapability {
            provider: "fake-hosted".to_string(),
            adapter_version: "0.1.0".to_string(),
            default_model: "fake-deterministic-v1".to_string(),
            routing: ProviderRoutingMetadata {
                route_key: "fake-hosted".to_string(),
                provider_kind: ProviderCapabilityKind::Hosted,
                fallback_priority: 100,
                enabled: true,
            },
            supported_models: vec![ModelCapability {
                model: "fake-deterministic-v1".to_string(),
                context_window_tokens: 8192,
                supports_json_output: true,
                supports_tool_calls: false,
            }],
            supports_streaming: true,
            supports_tool_calls: false,
            credential_ref_kinds: vec![crate::contracts::CredentialRefKind::SecretRef],
        }
    }

    fn complete(&self, request: &ScopedModelRequest) -> Result<ModelResponse, NormalizedError> {
        request.validate_boundary()?;
        self.capability().supported_model_for(request)?;
        if request
            .messages
            .iter()
            .any(|message| message.content.contains("error:rate_limit"))
        {
            return Err(rate_limit_error());
        }

        let content = deterministic_content(request);
        let usage = usage_for(request, &content);

        Ok(ModelResponse {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            model: request.model.clone(),
            content,
            finish_reason: FinishReason::Stop,
            usage,
        })
    }

    fn stream(&self, request: &ScopedModelRequest) -> Result<Vec<StreamFrame>, NormalizedError> {
        request.validate_boundary()?;
        self.capability().supported_model_for(request)?;
        if request
            .messages
            .iter()
            .any(|message| message.content.contains("error:rate_limit"))
        {
            return Ok(vec![
                StreamFrame {
                    request_id: request.request_id.clone(),
                    sequence: 0,
                    frame_type: StreamFrameType::Start,
                    delta: None,
                    usage: None,
                    error: None,
                },
                StreamFrame {
                    request_id: request.request_id.clone(),
                    sequence: 1,
                    frame_type: StreamFrameType::Error,
                    delta: None,
                    usage: None,
                    error: Some(rate_limit_error()),
                },
            ]);
        }

        let content = deterministic_content(request);
        let usage = usage_for(request, &content);
        let words: Vec<&str> = content.split_whitespace().collect();
        let mut frames = vec![StreamFrame {
            request_id: request.request_id.clone(),
            sequence: 0,
            frame_type: StreamFrameType::Start,
            delta: None,
            usage: None,
            error: None,
        }];

        for (index, word) in words.iter().enumerate() {
            frames.push(StreamFrame {
                request_id: request.request_id.clone(),
                sequence: (index + 1) as u32,
                frame_type: StreamFrameType::ContentDelta,
                delta: Some((*word).to_string()),
                usage: None,
                error: None,
            });
        }

        frames.push(StreamFrame {
            request_id: request.request_id.clone(),
            sequence: (words.len() + 1) as u32,
            frame_type: StreamFrameType::UsageDelta,
            delta: None,
            usage: Some(usage.clone()),
            error: None,
        });
        frames.push(StreamFrame {
            request_id: request.request_id.clone(),
            sequence: (words.len() + 2) as u32,
            frame_type: StreamFrameType::Final,
            delta: None,
            usage: Some(usage),
            error: None,
        });

        Ok(frames)
    }

    fn usage_event(
        &self,
        request: &ScopedModelRequest,
        response: Option<&ModelResponse>,
        _error: Option<&NormalizedError>,
    ) -> UsageEvent {
        let policy_decision_id = request.policy.policy_decision_id().to_string();
        let usage = response.map(|response| response.usage.clone());

        UsageEvent {
            id: "evt_01J9Z4P4BS0M9P2QJ6T8Z6W2EP".to_string(),
            event_type: "usage.gateway_request.recorded".to_string(),
            version: "0.1.0".to_string(),
            occurred_at: "2026-01-01T00:00:01.000Z".to_string(),
            source: EventSource::Gateway,
            working_group_id: request.working_group_id.clone(),
            actor: EventActorRef::from(&request.actor),
            resource: EventResourceRef {
                resource_type: "provider".to_string(),
                id: "prv_01J9Z4P4BS0M9P2QJ6T8Z6W2EP".to_string(),
            },
            correlation_id: request.correlation_id.clone(),
            request_id: request.request_id.clone(),
            policy_decision_id,
            idempotency_key: format!("usage_{}", request.request_id),
            payload: UsagePayload {
                subject: UsageSubject {
                    subject_type: UsageSubjectType::GatewayRequest,
                    id: request.request_id.clone(),
                },
                measurements: UsageMeasurements {
                    duration_ms: 0,
                    input_tokens: usage.as_ref().map(|usage| usage.input_tokens),
                    output_tokens: usage.as_ref().map(|usage| usage.output_tokens),
                    tool_invocations: Some(0),
                    estimated_cost_micros: Some(0),
                    metering_unit: None,
                    runtime_capability: None,
                },
            },
        }
    }
}

fn deterministic_content(request: &ScopedModelRequest) -> String {
    let prompt = request
        .messages
        .last()
        .map(|message| message.content.as_str())
        .unwrap_or_default();
    format!("fake:{}:{}", request.model, prompt.trim())
}

fn usage_for(request: &ScopedModelRequest, content: &str) -> UsageMeasurement {
    let input_tokens = request
        .messages
        .iter()
        .map(|message| count_tokens(&message.content))
        .sum();
    let output_tokens = count_tokens(content);
    UsageMeasurement {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens + output_tokens,
    }
}

fn count_tokens(value: &str) -> u32 {
    value.split_whitespace().count() as u32
}

fn rate_limit_error() -> NormalizedError {
    NormalizedError {
        code: NormalizedErrorCode::RateLimited,
        message: "The fake provider simulated a rate limit.".to_string(),
        retryable: true,
        upstream_status: Some(429),
        provider_error_class: Some("fake_rate_limit".to_string()),
    }
}
