use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    adapters::{
        ChatMessage, NormalizedProviderRequest, ProviderRef, ProviderResponse, ProviderRouter,
    },
    mcp::{resolve_endpoint, McpEndpoint, McpResolution},
    policy::{PolicyCheck, PolicyEngine, PolicySubject},
    usage::{GatewayRelayAuditEventV1, GatewayRelayReasonCode, UsageAttemptStatus},
};

#[derive(Clone)]
pub struct GatewayState {
    pub providers: Arc<ProviderRouter>,
    pub policy: Arc<dyn PolicyEngine>,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

pub async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Debug, Deserialize)]
pub struct AiRelayRequest {
    #[serde(default = "Uuid::new_v4")]
    pub request_id: Uuid,
    pub subject: PolicySubject,
    pub provider: ProviderRef,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct AiRelayResponse {
    pub response: ProviderResponse,
    pub gateway_relay_audit_event: GatewayRelayAuditEventV1,
}

pub async fn relay_ai_request(
    State(state): State<GatewayState>,
    Json(payload): Json<AiRelayRequest>,
) -> Result<Json<AiRelayResponse>, GatewayError> {
    let policy_check = PolicyCheck {
        subject: payload.subject.clone(),
        provider: payload.provider.clone(),
        operation: "ai.relay".to_string(),
    };
    let decision = state.policy.evaluate(policy_check).await;

    if !decision.allowed {
        let gateway_relay_audit_event = GatewayRelayAuditEventV1::with_reason_code(
            payload.request_id,
            payload.subject,
            payload.provider,
            decision.decision_id,
            UsageAttemptStatus::Denied,
            GatewayRelayReasonCode::PolicyDenied,
        );
        return Err(GatewayError::policy_denied(decision.reason)
            .with_gateway_relay_audit_event(gateway_relay_audit_event));
    }

    let normalized = NormalizedProviderRequest {
        request_id: payload.request_id,
        provider: payload.provider.clone(),
        messages: payload.messages,
        stream: payload.stream,
        timeout_ms: payload.timeout_ms,
    };
    let provider_response = match state.providers.route(normalized, decision.clone()).await {
        Ok(response) => response,
        Err(error) => {
            let gateway_relay_audit_event = GatewayRelayAuditEventV1::with_reason_code(
                payload.request_id,
                payload.subject,
                payload.provider,
                decision.decision_id,
                error.usage_status(),
                error.routing_reason_code(),
            );
            return Err(error.with_gateway_relay_audit_event(gateway_relay_audit_event));
        }
    };
    let gateway_relay_audit_event = GatewayRelayAuditEventV1::with_reason_code(
        payload.request_id,
        payload.subject,
        payload.provider,
        decision.decision_id,
        UsageAttemptStatus::Succeeded,
        provider_response.routing_reason_code,
    );

    Ok(Json(AiRelayResponse {
        response: provider_response,
        gateway_relay_audit_event,
    }))
}

pub async fn resolve_mcp_endpoint(Json(endpoint): Json<McpEndpoint>) -> Json<McpResolution> {
    Json(resolve_endpoint(endpoint))
}

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("{message}")]
    PolicyDenied {
        message: String,
        gateway_relay_audit_event: Option<GatewayRelayAuditEventV1>,
    },
    #[error("{message}")]
    Timeout {
        message: String,
        timeout_ms: Option<u64>,
        gateway_relay_audit_event: Option<GatewayRelayAuditEventV1>,
    },
}

impl GatewayError {
    pub fn policy_denied(reason: Option<String>) -> Self {
        Self::PolicyDenied {
            message: reason.unwrap_or_else(|| "request denied by policy".to_string()),
            gateway_relay_audit_event: None,
        }
    }

    pub fn timeout(message: impl Into<String>, timeout_ms: Option<u64>) -> Self {
        Self::Timeout {
            message: message.into(),
            timeout_ms,
            gateway_relay_audit_event: None,
        }
    }

    fn usage_status(&self) -> UsageAttemptStatus {
        match self {
            GatewayError::PolicyDenied { .. } => UsageAttemptStatus::Denied,
            GatewayError::Timeout { .. } => UsageAttemptStatus::Timeout,
        }
    }

    fn routing_reason_code(&self) -> GatewayRelayReasonCode {
        match self {
            GatewayError::PolicyDenied { .. } => GatewayRelayReasonCode::PolicyDenied,
            GatewayError::Timeout { .. } => GatewayRelayReasonCode::ProviderTimeout,
        }
    }

    fn with_gateway_relay_audit_event(
        self,
        gateway_relay_audit_event: GatewayRelayAuditEventV1,
    ) -> Self {
        match self {
            GatewayError::PolicyDenied { message, .. } => GatewayError::PolicyDenied {
                message,
                gateway_relay_audit_event: Some(gateway_relay_audit_event),
            },
            GatewayError::Timeout {
                message,
                timeout_ms,
                ..
            } => GatewayError::Timeout {
                message,
                timeout_ms,
                gateway_relay_audit_event: Some(gateway_relay_audit_event),
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: ErrorShape,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_relay_audit_event: Option<GatewayRelayAuditEventV1>,
}

#[derive(Debug, Serialize)]
pub struct ErrorShape {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, error, gateway_relay_audit_event) = match self {
            GatewayError::PolicyDenied {
                message,
                gateway_relay_audit_event,
            } => (
                StatusCode::FORBIDDEN,
                ErrorShape {
                    code: "policy_denied",
                    message,
                    retryable: false,
                    timeout_ms: None,
                },
                gateway_relay_audit_event,
            ),
            GatewayError::Timeout {
                message,
                timeout_ms,
                gateway_relay_audit_event,
            } => (
                StatusCode::GATEWAY_TIMEOUT,
                ErrorShape {
                    code: "provider_timeout",
                    message,
                    retryable: true,
                    timeout_ms,
                },
                gateway_relay_audit_event,
            ),
        };

        (
            status,
            Json(ErrorBody {
                error,
                gateway_relay_audit_event,
            }),
        )
            .into_response()
    }
}
