use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use taskotter_gateway::{
    adapters::ProviderKind,
    app,
    contracts::{
        validate_gateway_protocol_version, AuditEvent, GatewayHealth, GatewayRegistration,
        HealthStatus, McpHostingMode, NormalizedError, NormalizedErrorCode, PolicyInstruction,
        ProviderAdapterCapability, RuntimeFeatureFlags, ScopedCredentialRef,
        ScopedMcpSessionRequest, ScopedModelRequest, StreamFrame, StreamFrameType, UsageEvent,
        GATEWAY_PROTOCOL_VERSION,
    },
    mcp::McpRuntimeHost,
    mcp::{resolve_endpoint, McpEndpoint, McpHostMode},
    provider::{FakeProviderAdapter, ProviderAdapter},
};
use tower::ServiceExt;

async fn post_json(path: &str, body: Value) -> (StatusCode, Value) {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&body).unwrap();
    (status, json)
}

fn relay_payload(provider_id: &str) -> Value {
    json!({
        "subject": {
            "user_id": "user_1",
            "working_group_id": "wg_1",
            "agent_id": "agent_1"
        },
        "provider": {
            "provider_id": provider_id,
            "kind": "open_ai_compatible",
            "model": "test-model",
            "endpoint_id": "endpoint_1"
        },
        "messages": [
            { "role": "user", "content": "hello" }
        ],
        "stream": true
    })
}

#[tokio::test]
async fn routes_adapter_and_emits_usage_event() {
    let (status, body) = post_json("/v1/ai/relay", relay_payload("provider_1")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["response"]["provider_kind"], "open_ai_compatible");
    assert_eq!(body["response"]["stream_placeholder"], true);
    assert_eq!(
        body["usage_audit_event"]["schema_version"],
        "usage_audit_event.v1"
    );
    assert_eq!(body["usage_audit_event"]["status"], "succeeded");
    assert_eq!(
        body["usage_audit_event"]["decision_id"],
        "local-policy:ai.relay"
    );
}

#[tokio::test]
async fn policy_hook_denies_disabled_provider() {
    let (status, body) = post_json("/v1/ai/relay", relay_payload("disabled_provider")).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "policy_denied");
    assert_eq!(body["error"]["retryable"], false);
    assert_eq!(
        body["usage_audit_event"]["schema_version"],
        "usage_audit_event.v1"
    );
    assert_eq!(body["usage_audit_event"]["status"], "denied");
}

#[tokio::test]
async fn provider_timeout_has_stable_error_shape() {
    let mut payload = relay_payload("provider_1");
    payload["timeout_ms"] = json!(0);
    let (status, body) = post_json("/v1/ai/relay", payload).await;

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body["error"]["code"], "provider_timeout");
    assert_eq!(body["error"]["retryable"], true);
    assert_eq!(body["error"]["timeout_ms"], 0);
    assert_eq!(
        body["usage_audit_event"]["schema_version"],
        "usage_audit_event.v1"
    );
    assert_eq!(body["usage_audit_event"]["status"], "timeout");
}

#[test]
fn mcp_resolution_models_runner_hosted_mode_without_starting_runtime() {
    let resolution = resolve_endpoint(McpEndpoint {
        integration_id: "mcp_1".to_string(),
        host_mode: McpHostMode::RunnerHosted,
        runner_id: Some("runner_1".to_string()),
        remote_url: None,
        command: None,
    });

    assert_eq!(resolution.host_mode, McpHostMode::RunnerHosted);
    assert_eq!(resolution.lifecycle, "runner_dispatched_process");
    assert!(resolution.requires_policy_decision);
    assert!(!resolution.starts_runtime);
}

#[test]
fn provider_kind_serializes_as_contract_value() {
    let value = serde_json::to_value(ProviderKind::LocalRunner).unwrap();
    assert_eq!(value, json!("local_runner"));
}

fn fixture<T>(name: &str) -> T
where
    T: serde::de::DeserializeOwned,
{
    let path = format!("fixtures/gateway/v0_1/{name}.json");
    let bytes = std::fs::read(path).expect("fixture should exist");
    serde_json::from_slice(&bytes).expect("fixture should deserialize")
}

#[test]
fn gateway_registration_keeps_control_plane_relay_boundary() {
    let registration = GatewayRegistration {
        protocol_version: GATEWAY_PROTOCOL_VERSION.to_string(),
        gateway_id: "gw_local".to_string(),
        instance_id: "gw_local_001".to_string(),
        capabilities: vec![
            "provider.fake-hosted".to_string(),
            "mcp.health.placeholder".to_string(),
        ],
        control_plane_relay_only: true,
    };

    assert!(registration.control_plane_relay_only);
    assert_eq!(registration.protocol_version, "gateway.v0.1");
}

#[test]
fn fixtures_round_trip_and_validate_boundaries() {
    let model_request: ScopedModelRequest = fixture("scoped_model_request");
    let signed_model_request: ScopedModelRequest = fixture("scoped_model_signed_request");
    let mcp_request: ScopedMcpSessionRequest = fixture("scoped_mcp_session_request");
    let capability: ProviderAdapterCapability = fixture("provider_capability");
    let health: GatewayHealth = fixture("gateway_health");
    let frames: Vec<StreamFrame> = fixture("stream_frames");
    let usage: UsageEvent = fixture("usage_event");
    let audit: AuditEvent = fixture("audit_event");
    let hosted_mcp_usage: UsageEvent = fixture("hosted_mcp_denied_usage_event");
    let hosted_mcp_audit: AuditEvent = fixture("hosted_mcp_denied_audit_event");
    let error: NormalizedError = fixture("normalized_error");

    validate_gateway_protocol_version(&model_request.protocol_version).unwrap();
    validate_gateway_protocol_version(&signed_model_request.protocol_version).unwrap();
    validate_gateway_protocol_version(&mcp_request.protocol_version).unwrap();
    model_request.validate_boundary().unwrap();
    signed_model_request.validate_boundary().unwrap();
    mcp_request.validate_boundary().unwrap();
    assert_eq!(capability.provider, "fake-hosted");
    assert!(capability.supports_streaming);
    assert_eq!(health.status, HealthStatus::Ok);
    assert_eq!(frames.last().unwrap().frame_type, StreamFrameType::Final);
    assert_eq!(usage.event_type, "usage.gateway_request.recorded");
    assert_eq!(usage.version, "0.1.0");
    assert_eq!(
        usage.source,
        taskotter_gateway::contracts::EventSource::Gateway
    );
    assert_eq!(audit.event_type, "audit.policy_decision.denied");
    assert_eq!(audit.version, "0.1.0");
    assert_eq!(audit.payload.action, "gateway.provider.invoke");
    assert_eq!(
        hosted_mcp_usage
            .payload
            .measurements
            .runtime_capability
            .as_deref(),
        Some("gateway.hosted_mcp_billing")
    );
    assert_eq!(
        hosted_mcp_usage
            .payload
            .measurements
            .metering_unit
            .as_deref(),
        Some("hosted_mcp_runtime_ms")
    );
    assert_eq!(
        hosted_mcp_audit.payload.feature_flag.as_deref(),
        Some("gateway.hosted_mcp_billing.enabled")
    );
    assert_eq!(
        hosted_mcp_audit.payload.outcome,
        taskotter_gateway::contracts::AuditOutcome::Denied
    );
    assert_eq!(
        hosted_mcp_audit.payload.runtime_capability.as_deref(),
        Some("gateway.hosted_mcp_billing")
    );
    assert_eq!(
        hosted_mcp_audit.payload.approval_ref.as_deref(),
        Some("approval_required_before_paid_runtime")
    );
    assert_eq!(
        hosted_mcp_audit.policy_decision_id,
        "poldec_01J9Z4P4BS0M9P2QJ6T8Z6W2EP"
    );
    assert_eq!(error.code, NormalizedErrorCode::RateLimited);
}

#[test]
fn fake_provider_returns_deterministic_non_streaming_response_and_usage() {
    let adapter = FakeProviderAdapter::new();
    let request: ScopedModelRequest = fixture("scoped_model_request");

    let response = adapter.complete(&request).unwrap();
    let usage = adapter.usage_event(&request, Some(&response), None);

    assert_eq!(
        response.content,
        "fake:fake-deterministic-v1:summarize fixture contract"
    );
    assert_eq!(response.usage.input_tokens, 3);
    assert!(response.usage.output_tokens > 0);
    assert_eq!(usage.event_type, "usage.gateway_request.recorded");
    assert_eq!(
        usage.policy_decision_id,
        "poldec_01J9Z4P4BS0M9P2QJ6T8Z6W2EP"
    );
    assert_eq!(usage.payload.measurements.input_tokens, Some(3));
    assert_eq!(usage.payload.measurements.estimated_cost_micros, Some(0));
}

#[test]
fn signed_dispatch_provider_event_keeps_policy_decision_lineage_separate() {
    let adapter = FakeProviderAdapter::new();
    let request: ScopedModelRequest = fixture("scoped_model_signed_request");

    let response = adapter.complete(&request).unwrap();
    let usage = adapter.usage_event(&request, Some(&response), None);

    assert_eq!(
        usage.policy_decision_id,
        "poldec_01J9Z4P4BS0M9P2QJ6T8Z6W2EP"
    );
    assert!(!usage.policy_decision_id.starts_with("gwi_"));
}

#[test]
fn fake_provider_streams_ordered_frames_with_final_usage() {
    let adapter = FakeProviderAdapter::new();
    let request: ScopedModelRequest = fixture("scoped_model_request");

    let frames = adapter.stream(&request).unwrap();

    assert_eq!(frames.first().unwrap().frame_type, StreamFrameType::Start);
    assert_eq!(frames.last().unwrap().frame_type, StreamFrameType::Final);
    assert!(frames
        .iter()
        .any(|frame| frame.frame_type == StreamFrameType::ContentDelta));
    assert!(frames.last().unwrap().usage.is_some());

    for (expected, frame) in frames.iter().enumerate() {
        assert_eq!(frame.sequence, expected as u32);
    }
}

#[test]
fn fake_provider_normalizes_provider_errors() {
    let adapter = FakeProviderAdapter::new();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");
    request.messages[0].content = "error:rate_limit".to_string();

    let error = adapter.complete(&request).unwrap_err();
    let frames = adapter.stream(&request).unwrap();

    assert_eq!(error.code, NormalizedErrorCode::RateLimited);
    assert!(error.retryable);
    assert_eq!(
        frames.last().unwrap().error.as_ref().unwrap().code,
        NormalizedErrorCode::RateLimited
    );
}

#[test]
fn raw_credentials_are_rejected_before_adapter_execution() {
    let adapter = FakeProviderAdapter::new();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");
    request.credential_ref = ScopedCredentialRef {
        kind: taskotter_gateway::contracts::CredentialRefKind::SecretRef,
        reference: "sk-test-raw-secret".to_string(),
        scope: "wg_fixture/provider/fake-hosted".to_string(),
    };

    let error = adapter.complete(&request).unwrap_err();

    assert_eq!(error.code, NormalizedErrorCode::PolicyDenied);
    assert_eq!(
        error.provider_error_class.as_deref(),
        Some("raw_credential_value_rejected")
    );
}

#[test]
fn mcp_host_health_and_session_placeholder_are_verified() {
    let host = McpRuntimeHost::with_feature_flags(
        "mcp_host_local",
        RuntimeFeatureFlags {
            hosted_mcp_billing_enabled: true,
            provider_routing_enabled: false,
        },
    );
    let request: ScopedMcpSessionRequest = fixture("scoped_mcp_session_request");

    let capability = host.capability();
    let health = host.health(McpHostingMode::GatewayHosted);
    let session = host.open_session(&request).unwrap();
    let usage = host.usage_event(&request);
    let audit = host.audit_event(&request);

    assert_eq!(health.status, HealthStatus::Ok);
    assert!(capability
        .supported_hosting_modes
        .contains(&McpHostingMode::RunnerHosted));
    assert!(capability
        .high_risk_capabilities
        .iter()
        .any(
            |gate| gate.feature_flag == "gateway.hosted_mcp_billing.enabled"
                && gate.enabled
                && gate.default_policy_effect == "deny"
        ));
    assert_eq!(session.lifecycle_state, "ready_placeholder");
    assert_eq!(usage.event_type, "usage.gateway_request.recorded");
    assert_eq!(usage.payload.measurements.tool_invocations, Some(1));
    assert_eq!(
        usage.payload.measurements.runtime_capability.as_deref(),
        Some("gateway.hosted_mcp_billing")
    );
    assert_eq!(audit.payload.action, "gateway.mcp.session.open");
    assert_eq!(
        audit.payload.outcome,
        taskotter_gateway::contracts::AuditOutcome::Denied
    );
    assert_eq!(
        audit.payload.runtime_capability.as_deref(),
        Some("gateway.hosted_mcp_billing")
    );
    assert_eq!(
        audit.payload.feature_flag.as_deref(),
        Some("gateway.hosted_mcp_billing.enabled")
    );
    assert_eq!(
        audit.payload.approval_ref.as_deref(),
        Some("policy_decision_ref")
    );
    assert_eq!(
        audit.policy_decision_id,
        "poldec_01J9Z4P4BS0M9P2QJ6T8Z6W2EP"
    );
}

#[test]
fn hosted_mcp_runtime_is_disabled_by_default() {
    let host = McpRuntimeHost::new("mcp_host_local");
    let request: ScopedMcpSessionRequest = fixture("scoped_mcp_session_request");
    let capability = host.capability();

    let error = host.open_session(&request).unwrap_err();

    assert_eq!(error.code, NormalizedErrorCode::PolicyDenied);
    assert_eq!(
        error.provider_error_class.as_deref(),
        Some("feature_flag_disabled")
    );
    assert!(capability
        .high_risk_capabilities
        .iter()
        .all(|gate| !gate.enabled && gate.default_policy_effect == "deny"));
}

#[test]
fn signed_dispatch_mcp_events_keep_policy_decision_lineage_separate() {
    let host = McpRuntimeHost::new("mcp_host_local");
    let mut request: ScopedMcpSessionRequest = fixture("scoped_mcp_session_request");
    request.policy = PolicyInstruction::SignedDispatchPlaceholder {
        instruction_ref: "gwi_01J9Z4P4BS0M9P2QJ6T8Z6W2EP".to_string(),
        signature_ref: "sigref_01J9Z4P4BS0M9P2QJ6T8Z6W2EP".to_string(),
        policy_decision_id: "poldec_01J9Z4P4BS0M9P2QJ6T8Z6W2EP".to_string(),
        expires_at: "2026-06-01T00:00:00Z".to_string(),
    };

    let usage = host.usage_event(&request);
    let audit = host.audit_event(&request);

    assert_eq!(
        usage.policy_decision_id,
        "poldec_01J9Z4P4BS0M9P2QJ6T8Z6W2EP"
    );
    assert_eq!(
        audit.policy_decision_id,
        "poldec_01J9Z4P4BS0M9P2QJ6T8Z6W2EP"
    );
    assert_eq!(
        audit.payload.outcome,
        taskotter_gateway::contracts::AuditOutcome::Denied
    );
    assert_eq!(
        audit.payload.runtime_capability.as_deref(),
        Some("gateway.hosted_mcp_billing")
    );
    assert_eq!(
        audit.payload.feature_flag.as_deref(),
        Some("gateway.hosted_mcp_billing.enabled")
    );
    assert_eq!(
        audit.payload.approval_ref.as_deref(),
        Some("policy_decision_ref")
    );
    assert!(!usage.policy_decision_id.starts_with("gwi_"));
    assert!(!audit.policy_decision_id.starts_with("gwi_"));
}

#[test]
fn rejects_unsupported_gateway_protocol_fixture() {
    let bytes = std::fs::read("fixtures/gateway/unsupported_protocol/scoped_model_request.json")
        .expect("unsupported protocol fixture should exist");
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("unsupported fixture should parse");
    let version = value["protocol_version"]
        .as_str()
        .expect("fixture must declare a protocol version");

    let error = validate_gateway_protocol_version(version).unwrap_err();

    assert_eq!(error.code, NormalizedErrorCode::InvalidGatewayRequest);
    assert_eq!(
        error.provider_error_class.as_deref(),
        Some("unsupported_gateway_protocol_version")
    );
}

#[test]
fn contract_compatibility_matrix_declares_supported_versions() {
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("../contract-compatibility.json"))
            .expect("compatibility declaration should parse");
    let versions = matrix["gateway"]["supported_protocol_versions"]
        .as_array()
        .expect("supported protocol versions must be declared");
    let event_versions = matrix["gateway"]["event_envelope_versions"]
        .as_array()
        .expect("event envelope versions must be declared");

    assert!(versions
        .iter()
        .any(|version| version == GATEWAY_PROTOCOL_VERSION));
    assert!(event_versions.iter().any(|version| version == "0.1.0"));
}
