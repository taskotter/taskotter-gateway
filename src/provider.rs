use crate::contracts::{
    AuditEvent, AuditOutcome, AuditPayload, CredentialRefKind, EventActorRef, EventResourceRef,
    EventSource, FinishReason, GatewayCapabilityGate, HealthStatus, HighRiskGatewayCapability,
    ModelCapability, ModelResponse, NormalizedError, NormalizedErrorCode,
    ProviderAdapterCapability, ProviderAdapterHealth, ScopedModelRequest, StreamFrame,
    StreamFrameType, UsageEvent, UsageMeasurement, UsageMeasurements, UsagePayload, UsageSubject,
    UsageSubjectType,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const PROVIDER_RESOURCE_ID: &str = "prv_01J9Z4P4BS0M9P2QJ6T8Z6W2EP";
const EVENT_ID_USAGE: &str = "evt_01J9Z4P4BS0M9P2QJ6T8Z6W2EP";
const EVENT_ID_AUDIT: &str = "evt_01J9Z4P4BS0M9P2QJ6T8Z6W2EQ";

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

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProviderAdapter {
    provider: String,
    adapter_version: String,
    supported_models: Vec<ModelCapability>,
}

impl OpenAiCompatibleProviderAdapter {
    pub fn new(provider: impl Into<String>, supported_models: Vec<ModelCapability>) -> Self {
        Self {
            provider: provider.into(),
            adapter_version: "0.1.0".to_string(),
            supported_models,
        }
    }

    pub fn health(&self) -> ProviderAdapterHealth {
        ProviderAdapterHealth {
            provider: self.provider.clone(),
            status: HealthStatus::Ok,
            supports_streaming: true,
        }
    }

    pub fn routing_gate(&self, enabled: bool) -> GatewayCapabilityGate {
        GatewayCapabilityGate {
            capability: HighRiskGatewayCapability::SensitiveProviderRouting,
            feature_flag: HighRiskGatewayCapability::SensitiveProviderRouting
                .feature_flag()
                .to_string(),
            enabled,
            default_policy_effect: "deny".to_string(),
        }
    }

    pub fn build_chat_completions_request(
        &self,
        request: &ScopedModelRequest,
    ) -> Result<OpenAiCompatibleChatRequest, NormalizedError> {
        request.validate_boundary()?;
        let mut body = json!({
            "model": request.model,
            "messages": request
                .messages
                .iter()
                .map(|message| json!({
                    "role": message.role,
                    "content": message.content,
                }))
                .collect::<Vec<_>>(),
            "stream": request.stream,
            "metadata": {
                "gateway_request_id": request.request_id,
                "gateway_correlation_id": request.correlation_id,
                "working_group_id": request.working_group_id,
            },
        });

        if request.stream {
            body["stream_options"] = json!({
                "include_usage": true,
            });
        }

        Ok(OpenAiCompatibleChatRequest {
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            credential_ref: request.credential_ref.reference.clone(),
            body,
        })
    }

    pub fn complete_from_response(
        &self,
        request: &ScopedModelRequest,
        response: OpenAiCompatibleHttpResponse,
    ) -> Result<ModelResponse, NormalizedError> {
        if response.status >= 400 {
            return Err(normalize_openai_error(response.status, &response.body));
        }

        let choice = response
            .body
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .ok_or_else(malformed_response)?;
        let content = choice
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(malformed_response)?;
        let finish_reason = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(openai_finish_reason)
            .unwrap_or(FinishReason::Stop);
        let usage = usage_from_openai(&response.body)
            .unwrap_or_else(|| usage_for_content(request, content));

        Ok(ModelResponse {
            request_id: request.request_id.clone(),
            provider: request.provider.clone(),
            model: response
                .body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or(&request.model)
                .to_string(),
            content: content.to_string(),
            finish_reason,
            usage,
        })
    }

    pub fn stream_from_events(
        &self,
        request: &ScopedModelRequest,
        events: &[OpenAiCompatibleStreamEvent],
    ) -> Result<Vec<StreamFrame>, NormalizedError> {
        request.validate_boundary()?;

        let mut frames = vec![StreamFrame {
            request_id: request.request_id.clone(),
            sequence: 0,
            frame_type: StreamFrameType::Start,
            delta: None,
            usage: None,
            error: None,
        }];

        let mut final_usage = None;
        let mut saw_final = false;
        for event in events {
            if let Some(error) = &event.error {
                frames.push(StreamFrame {
                    request_id: request.request_id.clone(),
                    sequence: frames.len() as u32,
                    frame_type: StreamFrameType::Error,
                    delta: None,
                    usage: None,
                    error: Some(normalize_openai_error(event.status.unwrap_or(500), error)),
                });
                return Ok(frames);
            }

            if let Some(usage) = event.usage.as_ref().and_then(usage_from_openai_value) {
                final_usage = Some(usage.clone());
                frames.push(StreamFrame {
                    request_id: request.request_id.clone(),
                    sequence: frames.len() as u32,
                    frame_type: StreamFrameType::UsageDelta,
                    delta: None,
                    usage: Some(usage),
                    error: None,
                });
            }

            for choice in &event.choices {
                if let Some(delta) = choice.delta.content.as_deref() {
                    frames.push(StreamFrame {
                        request_id: request.request_id.clone(),
                        sequence: frames.len() as u32,
                        frame_type: StreamFrameType::ContentDelta,
                        delta: Some(delta.to_string()),
                        usage: None,
                        error: None,
                    });
                }
                if choice.finish_reason.is_some() {
                    saw_final = true;
                }
            }
        }

        let usage = final_usage.unwrap_or_else(|| usage_for_content(request, ""));
        frames.push(StreamFrame {
            request_id: request.request_id.clone(),
            sequence: frames.len() as u32,
            frame_type: StreamFrameType::Final,
            delta: None,
            usage: Some(usage),
            error: None,
        });

        if !saw_final {
            frames.last_mut().unwrap().error = Some(NormalizedError {
                code: NormalizedErrorCode::MalformedUpstreamResponse,
                message: "OpenAI-compatible stream ended without a finish reason.".to_string(),
                retryable: true,
                upstream_status: None,
                provider_error_class: Some("openai_stream_missing_finish_reason".to_string()),
            });
        }

        Ok(frames)
    }

    pub fn audit_event(&self, request: &ScopedModelRequest, outcome: AuditOutcome) -> AuditEvent {
        AuditEvent {
            id: EVENT_ID_AUDIT.to_string(),
            event_type: "audit.provider_request.completed".to_string(),
            version: "0.1.0".to_string(),
            occurred_at: "2026-01-01T00:00:02.000Z".to_string(),
            source: EventSource::Gateway,
            working_group_id: request.working_group_id.clone(),
            actor: EventActorRef::from(&request.actor),
            resource: EventResourceRef {
                resource_type: "provider".to_string(),
                id: self.provider.clone(),
            },
            correlation_id: request.correlation_id.clone(),
            request_id: request.request_id.clone(),
            policy_decision_id: request.policy.policy_decision_id().to_string(),
            payload: AuditPayload {
                action: "gateway.provider.invoke".to_string(),
                outcome,
                runtime_capability: Some(
                    HighRiskGatewayCapability::SensitiveProviderRouting
                        .contract_name()
                        .to_string(),
                ),
                feature_flag: Some(
                    HighRiskGatewayCapability::SensitiveProviderRouting
                        .feature_flag()
                        .to_string(),
                ),
                approval_ref: Some("policy_decision_ref".to_string()),
            },
        }
    }
}

impl Default for OpenAiCompatibleProviderAdapter {
    fn default() -> Self {
        Self::new(
            "openai-compatible",
            vec![ModelCapability {
                model: "gpt-compatible-test".to_string(),
                context_window_tokens: 128_000,
                supports_json_output: true,
                supports_tool_calls: true,
            }],
        )
    }
}

impl ProviderAdapter for OpenAiCompatibleProviderAdapter {
    fn capability(&self) -> ProviderAdapterCapability {
        ProviderAdapterCapability {
            provider: self.provider.clone(),
            adapter_version: self.adapter_version.clone(),
            supported_models: self.supported_models.clone(),
            supports_streaming: true,
            supports_tool_calls: true,
            credential_ref_kinds: vec![CredentialRefKind::SecretRef],
        }
    }

    fn complete(&self, request: &ScopedModelRequest) -> Result<ModelResponse, NormalizedError> {
        self.build_chat_completions_request(request)?;
        let content = format!(
            "openai-compatible:{}:{}",
            request.model,
            request
                .messages
                .last()
                .map(|message| message.content.trim())
                .unwrap_or_default()
        );

        self.complete_from_response(
            request,
            OpenAiCompatibleHttpResponse {
                status: 200,
                body: json!({
                    "id": "chatcmpl_fixture",
                    "object": "chat.completion",
                    "model": request.model,
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": content,
                        },
                        "finish_reason": "stop",
                    }],
                    "usage": {
                        "prompt_tokens": request
                            .messages
                            .iter()
                            .map(|message| count_tokens(&message.content))
                            .sum::<u32>(),
                        "completion_tokens": count_tokens(&content),
                        "total_tokens": request
                            .messages
                            .iter()
                            .map(|message| count_tokens(&message.content))
                            .sum::<u32>() + count_tokens(&content),
                    },
                }),
            },
        )
    }

    fn stream(&self, request: &ScopedModelRequest) -> Result<Vec<StreamFrame>, NormalizedError> {
        self.build_chat_completions_request(request)?;
        let content = format!(
            "openai-compatible:{}",
            request
                .messages
                .last()
                .map(|message| message.content.trim())
                .unwrap_or_default()
        );
        let event = OpenAiCompatibleStreamEvent {
            choices: vec![OpenAiCompatibleStreamChoice {
                delta: OpenAiCompatibleStreamDelta {
                    content: Some(content),
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(json!({
                "prompt_tokens": request
                    .messages
                    .iter()
                    .map(|message| count_tokens(&message.content))
                    .sum::<u32>(),
                "completion_tokens": 1,
                "total_tokens": request
                    .messages
                    .iter()
                    .map(|message| count_tokens(&message.content))
                    .sum::<u32>() + 1,
            })),
            error: None,
            status: None,
        };

        self.stream_from_events(request, &[event])
    }

    fn usage_event(
        &self,
        request: &ScopedModelRequest,
        response: Option<&ModelResponse>,
        error: Option<&NormalizedError>,
    ) -> UsageEvent {
        provider_usage_event(
            request,
            &self.provider,
            response.map(|response| response.usage.clone()),
            error,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiCompatibleChatRequest {
    pub method: String,
    pub path: String,
    pub credential_ref: String,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiCompatibleHttpResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiCompatibleStreamEvent {
    #[serde(default)]
    pub choices: Vec<OpenAiCompatibleStreamChoice>,
    #[serde(default)]
    pub usage: Option<Value>,
    #[serde(default)]
    pub error: Option<Value>,
    #[serde(default)]
    pub status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiCompatibleStreamChoice {
    #[serde(default)]
    pub delta: OpenAiCompatibleStreamDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiCompatibleStreamDelta {
    #[serde(default)]
    pub content: Option<String>,
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
            id: EVENT_ID_USAGE.to_string(),
            event_type: "usage.gateway_request.recorded".to_string(),
            version: "0.1.0".to_string(),
            occurred_at: "2026-01-01T00:00:01.000Z".to_string(),
            source: EventSource::Gateway,
            working_group_id: request.working_group_id.clone(),
            actor: EventActorRef::from(&request.actor),
            resource: EventResourceRef {
                resource_type: "provider".to_string(),
                id: PROVIDER_RESOURCE_ID.to_string(),
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

fn usage_for_content(request: &ScopedModelRequest, content: &str) -> UsageMeasurement {
    usage_for(request, content)
}

fn provider_usage_event(
    request: &ScopedModelRequest,
    provider_id: &str,
    usage: Option<UsageMeasurement>,
    error: Option<&NormalizedError>,
) -> UsageEvent {
    UsageEvent {
        id: EVENT_ID_USAGE.to_string(),
        event_type: "usage.gateway_request.recorded".to_string(),
        version: "0.1.0".to_string(),
        occurred_at: "2026-01-01T00:00:01.000Z".to_string(),
        source: EventSource::Gateway,
        working_group_id: request.working_group_id.clone(),
        actor: EventActorRef::from(&request.actor),
        resource: EventResourceRef {
            resource_type: "provider".to_string(),
            id: provider_id.to_string(),
        },
        correlation_id: request.correlation_id.clone(),
        request_id: request.request_id.clone(),
        policy_decision_id: request.policy.policy_decision_id().to_string(),
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
                estimated_cost_micros: Some(if error.is_some() { 0 } else { 1 }),
                metering_unit: Some("provider_request".to_string()),
                runtime_capability: Some(
                    HighRiskGatewayCapability::SensitiveProviderRouting
                        .contract_name()
                        .to_string(),
                ),
            },
        },
    }
}

fn usage_from_openai(body: &Value) -> Option<UsageMeasurement> {
    body.get("usage").and_then(usage_from_openai_value)
}

fn usage_from_openai_value(value: &Value) -> Option<UsageMeasurement> {
    let input_tokens = value.get("prompt_tokens")?.as_u64()? as u32;
    let output_tokens = value.get("completion_tokens")?.as_u64()? as u32;
    let total_tokens = value
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or((input_tokens + output_tokens) as u64) as u32;

    Some(UsageMeasurement {
        input_tokens,
        output_tokens,
        total_tokens,
    })
}

fn openai_finish_reason(value: &str) -> FinishReason {
    match value {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "tool_calls" | "function_call" => FinishReason::ToolCall,
        "content_filter" => FinishReason::Refusal,
        _ => FinishReason::Error,
    }
}

fn normalize_openai_error(status: u16, body: &Value) -> NormalizedError {
    let error = body.get("error").unwrap_or(body);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("OpenAI-compatible provider returned an error.")
        .to_string();
    let provider_error_class = error
        .get("type")
        .or_else(|| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("openai_compatible_error")
        .to_string();
    let code = match status {
        408 | 504 => NormalizedErrorCode::UpstreamTimeout,
        429 => NormalizedErrorCode::RateLimited,
        500..=599 => NormalizedErrorCode::UpstreamUnavailable,
        _ => NormalizedErrorCode::MalformedUpstreamResponse,
    };

    NormalizedError {
        code,
        message,
        retryable: matches!(status, 408 | 429 | 500..=599),
        upstream_status: Some(status),
        provider_error_class: Some(provider_error_class),
    }
}

fn malformed_response() -> NormalizedError {
    NormalizedError {
        code: NormalizedErrorCode::MalformedUpstreamResponse,
        message: "OpenAI-compatible provider response did not match expected chat shape."
            .to_string(),
        retryable: true,
        upstream_status: None,
        provider_error_class: Some("openai_chat_response_malformed".to_string()),
    }
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
