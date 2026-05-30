use taskotter_gateway::contracts::{
    AuditEvent, GatewayHealth, GatewayRegistration, HealthStatus, McpHostingMode, NormalizedError,
    NormalizedErrorCode, ProviderAdapterCapability, ScopedCredentialRef, ScopedMcpSessionRequest,
    ScopedModelRequest, StreamFrame, StreamFrameType, UsageEvent, GATEWAY_PROTOCOL_VERSION,
};
use taskotter_gateway::mcp::McpRuntimeHost;
use taskotter_gateway::provider::{FakeProviderAdapter, ProviderAdapter};

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
    let error: NormalizedError = fixture("normalized_error");

    model_request.validate_boundary().unwrap();
    signed_model_request.validate_boundary().unwrap();
    mcp_request.validate_boundary().unwrap();
    assert_eq!(capability.provider, "fake-hosted");
    assert!(capability.supports_streaming);
    assert_eq!(health.status, HealthStatus::Ok);
    assert_eq!(frames.last().unwrap().frame_type, StreamFrameType::Final);
    assert_eq!(usage.schema_version, "usage-event@0.1.0");
    assert_eq!(
        usage.source,
        taskotter_gateway::contracts::EventSource::Gateway
    );
    assert_eq!(audit.schema_version, "audit-event@0.1.0");
    assert_eq!(audit.action, "gateway.provider.invoke");
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
    assert_eq!(usage.schema_version, "usage-event@0.1.0");
    assert_eq!(
        usage.policy_decision_id.as_deref(),
        Some("poldec_01J9Z4P4BS0M9P2QJ6T8Z6W2EP")
    );
    assert_eq!(usage.measurements.input_tokens, Some(3));
    assert_eq!(usage.measurements.estimated_cost_micros, Some(0));
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
    let host = McpRuntimeHost::new("mcp_host_local");
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
    assert_eq!(session.lifecycle_state, "ready_placeholder");
    assert_eq!(usage.schema_version, "usage-event@0.1.0");
    assert_eq!(usage.measurements.tool_invocations, Some(1));
    assert_eq!(audit.action, "gateway.mcp.session.open");
}
