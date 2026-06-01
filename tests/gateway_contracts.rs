use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use taskotter_gateway::{
    adapters::ProviderKind,
    app,
    contracts::{
        validate_gateway_protocol_version, AlphaNormalizedContractSnapshot, AuditEvent,
        GatewayHealth, GatewayRegistration, HealthStatus, McpHostingMode, NormalizedError,
        NormalizedErrorCode, PolicyInstruction, ProviderAdapterCapability, RouteType,
        RoutingReasonCode, RuntimeFeatureFlags, ScopedCredentialRef, ScopedMcpSessionRequest,
        ScopedModelRequest, StreamFrame, StreamFrameType, UsageEvent, GATEWAY_PROTOCOL_VERSION,
    },
    mcp::McpRuntimeHost,
    mcp::{resolve_endpoint, McpEndpoint, McpHostMode},
    provider::{FakeProviderAdapter, ProviderAdapter},
    provider::{
        OpenAiCompatibleHttpResponse, OpenAiCompatibleProviderAdapter, OpenAiCompatibleStreamEvent,
    },
    simulator::GatewaySimulationFixture,
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
    let snapshot: AlphaNormalizedContractSnapshot = fixture("alpha_normalized_contract_snapshot");
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
    assert_eq!(capability.default_model, "fake-deterministic-v1");
    assert_eq!(capability.routing.route_key, "fake-hosted");
    assert_eq!(capability.routing.fallback_priority, 100);
    assert!(capability.routing.enabled);
    assert!(capability.supports_streaming);
    assert!(capability
        .routing_reason_codes
        .contains(&RoutingReasonCode::ExplicitSelection));
    assert_eq!(health.status, HealthStatus::Ok);
    assert_eq!(frames.last().unwrap().frame_type, StreamFrameType::Final);
    assert!(frames
        .first()
        .unwrap()
        .routing
        .as_ref()
        .is_some_and(|routing| routing.reason_code == RoutingReasonCode::ExplicitSelection));
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
    assert_eq!(snapshot.protocol_version, GATEWAY_PROTOCOL_VERSION);
    assert_eq!(
        snapshot.response.routing.reason_code,
        RoutingReasonCode::FallbackAfterError
    );
    assert_eq!(
        snapshot
            .usage
            .payload
            .routing
            .as_ref()
            .expect("usage event must carry routing lineage")
            .reason_code,
        RoutingReasonCode::FallbackAfterError
    );
    assert_eq!(
        snapshot
            .audit
            .payload
            .routing
            .as_ref()
            .expect("audit event must carry routing lineage")
            .fallback_from_provider
            .as_deref(),
        Some("fake-hosted-primary")
    );
    assert!(snapshot
        .extension_points
        .contains(&taskotter_gateway::contracts::ContractExtensionPoint::UsageMeasurements));
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
    assert_eq!(response.protocol_version, GATEWAY_PROTOCOL_VERSION);
    assert_eq!(response.correlation_id, request.correlation_id);
    assert_eq!(
        response.routing.reason_code,
        RoutingReasonCode::ExplicitSelection
    );
    assert!(response.usage.output_tokens > 0);
    assert_eq!(usage.event_type, "usage.gateway_request.recorded");
    assert_eq!(
        usage.policy_decision_id,
        "poldec_01J9Z4P4BS0M9P2QJ6T8Z6W2EP"
    );
    assert_eq!(usage.payload.measurements.input_tokens, Some(3));
    assert_eq!(usage.payload.measurements.estimated_cost_micros, Some(0));
    assert_eq!(
        usage.payload.routing.as_ref().unwrap().selected_provider,
        "fake-hosted"
    );
    assert_eq!(
        usage.payload.routing.as_ref().unwrap().reason_code,
        RoutingReasonCode::ExplicitSelection
    );
}

#[test]
fn provider_capability_metadata_is_the_adapter_route_boundary() {
    let adapter = FakeProviderAdapter::new();
    let capability = adapter.capability();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");

    let model = capability.supported_model_for(&request).unwrap();
    assert_eq!(model.model, "fake-deterministic-v1");

    request.model = "future-model".to_string();
    let error = capability.supported_model_for(&request).unwrap_err();
    assert_eq!(error.code, NormalizedErrorCode::PolicyDenied);
    assert_eq!(
        error.provider_error_class.as_deref(),
        Some("provider_model_not_supported")
    );
}

#[test]
fn fake_provider_rejects_requests_outside_capability_metadata() {
    let adapter = FakeProviderAdapter::new();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");
    request.provider = "unknown-provider".to_string();

    let error = adapter.complete(&request).unwrap_err();

    assert_eq!(error.code, NormalizedErrorCode::PolicyDenied);
    assert_eq!(
        error.provider_error_class.as_deref(),
        Some("provider_capability_mismatch")
    );
}

#[test]
fn fake_provider_complete_rejects_unsupported_credential_ref_kind() {
    let adapter = FakeProviderAdapter::new();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");
    request.credential_ref.kind =
        taskotter_gateway::contracts::CredentialRefKind::RunnerJobCredentialRef;

    let error = adapter.complete(&request).unwrap_err();

    assert_eq!(error.code, NormalizedErrorCode::PolicyDenied);
    assert_eq!(
        error.provider_error_class.as_deref(),
        Some("provider_credential_ref_kind_not_supported")
    );
}

#[test]
fn fake_provider_stream_rejects_unsupported_credential_ref_kind() {
    let adapter = FakeProviderAdapter::new();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");
    request.credential_ref.kind =
        taskotter_gateway::contracts::CredentialRefKind::ExternalMcpCredentialRef;

    let error_frame = adapter
        .stream(&request)
        .expect_err("unsupported credential ref kind must fail before streaming");

    assert_eq!(error_frame.code, NormalizedErrorCode::PolicyDenied);
    assert_eq!(
        error_frame.provider_error_class.as_deref(),
        Some("provider_credential_ref_kind_not_supported")
    );
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
fn openai_compatible_adapter_builds_request_without_raw_credentials() {
    let adapter = OpenAiCompatibleProviderAdapter::default();
    let request: ScopedModelRequest = fixture("scoped_model_request");

    let provider_request = adapter.build_chat_completions_request(&request).unwrap();

    assert_eq!(provider_request.method, "POST");
    assert_eq!(provider_request.path, "/v1/chat/completions");
    assert_eq!(
        provider_request.credential_ref,
        "secret_ref_provider_fake_fixture"
    );
    assert_eq!(provider_request.body["model"], request.model);
    assert_eq!(provider_request.body["stream"], request.stream);
    assert_eq!(
        provider_request.body["stream_options"]["include_usage"],
        true
    );
    assert!(provider_request.body.get("metadata").is_none());
    assert!(
        !provider_request
            .body
            .to_string()
            .contains(&request.request_id)
            && !provider_request
                .body
                .to_string()
                .contains(&request.correlation_id)
            && !provider_request
                .body
                .to_string()
                .contains(&request.working_group_id),
        "provider body must not expose internal request/correlation/workspace lineage"
    );
    assert!(
        !provider_request
            .body
            .to_string()
            .contains("secret_ref_provider_fake_fixture"),
        "credential references stay outside the provider JSON body"
    );
}

#[test]
fn openai_compatible_non_stream_request_omits_stream_options() {
    let adapter = OpenAiCompatibleProviderAdapter::default();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");
    request.stream = false;

    let provider_request = adapter.build_chat_completions_request(&request).unwrap();

    assert_eq!(provider_request.body["stream"], false);
    assert!(provider_request.body.get("stream_options").is_none());
}

#[test]
fn openai_compatible_stream_request_includes_usage_stream_options() {
    let adapter = OpenAiCompatibleProviderAdapter::default();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");
    request.stream = true;

    let provider_request = adapter.build_chat_completions_request(&request).unwrap();

    assert_eq!(provider_request.body["stream"], true);
    assert_eq!(
        provider_request.body["stream_options"]["include_usage"],
        true
    );
}

#[test]
fn openai_compatible_adapter_maps_response_usage_and_audit() {
    let adapter = OpenAiCompatibleProviderAdapter::default();
    let request: ScopedModelRequest = fixture("scoped_model_request");
    let response = adapter
        .complete_from_response(
            &request,
            OpenAiCompatibleHttpResponse {
                status: 200,
                body: fixture("openai_chat_completion_response"),
            },
        )
        .unwrap();
    let usage = adapter.usage_event(&request, Some(&response), None);
    let audit = adapter.audit_event(
        &request,
        taskotter_gateway::contracts::AuditOutcome::Succeeded,
    );

    assert_eq!(response.content, "adapter normalized response");
    assert_eq!(
        response.finish_reason,
        taskotter_gateway::contracts::FinishReason::Stop
    );
    assert_eq!(response.usage.input_tokens, 11);
    assert_eq!(response.usage.output_tokens, 3);
    assert_eq!(usage.resource.id, "openai-compatible");
    assert_eq!(
        usage.payload.measurements.runtime_capability.as_deref(),
        Some("gateway.sensitive_provider_routing")
    );
    assert_eq!(audit.payload.action, "gateway.provider.invoke");
    assert_eq!(
        audit.payload.feature_flag.as_deref(),
        Some("gateway.provider_routing.enabled")
    );
}

#[test]
fn openai_compatible_adapter_maps_stream_and_provider_errors() {
    let adapter = OpenAiCompatibleProviderAdapter::default();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");
    request.stream = true;
    let events: Vec<OpenAiCompatibleStreamEvent> = fixture("openai_chat_completion_stream");

    let frames = adapter.stream_from_events(&request, &events).unwrap();
    let error = adapter
        .complete_from_response(
            &request,
            OpenAiCompatibleHttpResponse {
                status: 429,
                body: fixture("openai_rate_limit_error"),
            },
        )
        .unwrap_err();

    assert_eq!(frames.first().unwrap().frame_type, StreamFrameType::Start);
    assert!(frames
        .iter()
        .any(|frame| frame.frame_type == StreamFrameType::ContentDelta
            && frame.delta.as_deref() == Some("adapter ")));
    assert!(frames
        .iter()
        .any(|frame| frame.frame_type == StreamFrameType::UsageDelta));
    assert_eq!(frames.last().unwrap().frame_type, StreamFrameType::Final);
    assert_eq!(
        frames.last().unwrap().usage.as_ref().unwrap().total_tokens,
        14
    );
    assert_eq!(error.code, NormalizedErrorCode::RateLimited);
    assert_eq!(error.upstream_status, Some(429));
    assert_eq!(
        error.message,
        "OpenAI-compatible provider rate limited the request."
    );
    assert_eq!(error.provider_error_class.as_deref(), Some("rate_limited"));
}

#[test]
fn openai_compatible_provider_errors_are_sanitized() {
    let adapter = OpenAiCompatibleProviderAdapter::default();
    let request: ScopedModelRequest = fixture("scoped_model_request");
    let sensitive_error = serde_json::json!({
        "error": {
            "message": "project proj_internal_123 saw prompt token sk-test-secret for wg_01J9Z4P4BS0M9P2QJ6T8Z6W2EP",
            "type": "rate_limit_error",
            "code": "acct_internal_456"
        }
    });

    let error = adapter
        .complete_from_response(
            &request,
            OpenAiCompatibleHttpResponse {
                status: 429,
                body: sensitive_error,
            },
        )
        .unwrap_err();
    let serialized = serde_json::to_string(&error).unwrap();

    assert_eq!(error.code, NormalizedErrorCode::RateLimited);
    assert_eq!(
        error.message,
        "OpenAI-compatible provider rate limited the request."
    );
    assert_eq!(error.provider_error_class.as_deref(), Some("rate_limited"));
    assert!(!serialized.contains("proj_internal_123"));
    assert!(!serialized.contains("sk-test-secret"));
    assert!(!serialized.contains("wg_01J9Z4P4BS0M9P2QJ6T8Z6W2EP"));
    assert!(!serialized.contains("acct_internal_456"));
}

#[test]
fn openai_compatible_stream_errors_are_sanitized() {
    let adapter = OpenAiCompatibleProviderAdapter::default();
    let mut request: ScopedModelRequest = fixture("scoped_model_request");
    request.stream = true;
    let events = vec![OpenAiCompatibleStreamEvent {
        choices: vec![],
        usage: None,
        error: Some(serde_json::json!({
            "message": "bearer token leaked for corr_01J9Z4P4BS0M9P2QJ6T8Z6W2EP",
            "type": "server_error",
            "code": "runner_secret_ref_123"
        })),
        status: Some(500),
    }];

    let frames = adapter.stream_from_events(&request, &events).unwrap();
    let error = frames.last().unwrap().error.as_ref().unwrap();
    let serialized = serde_json::to_string(error).unwrap();

    assert_eq!(error.code, NormalizedErrorCode::UpstreamUnavailable);
    assert_eq!(
        error.message,
        "OpenAI-compatible provider is temporarily unavailable."
    );
    assert_eq!(
        error.provider_error_class.as_deref(),
        Some("upstream_error")
    );
    assert!(!serialized.contains("bearer token"));
    assert!(!serialized.contains("corr_01J9Z4P4BS0M9P2QJ6T8Z6W2EP"));
    assert!(!serialized.contains("runner_secret_ref_123"));
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

#[test]
fn alpha_snapshot_rejects_unknown_contract_fields() {
    let mut snapshot: Value = fixture("alpha_normalized_contract_snapshot");
    snapshot["response"]["unexpected_downstream_field"] = json!("must be modeled explicitly");

    let error = serde_json::from_value::<AlphaNormalizedContractSnapshot>(snapshot).unwrap_err();

    assert!(error.to_string().contains("unexpected_downstream_field"));
}

#[test]
fn alpha_contract_uses_parent_canonical_routing_lineage() {
    let capability: ProviderAdapterCapability = fixture("provider_capability");
    let snapshot: AlphaNormalizedContractSnapshot = fixture("alpha_normalized_contract_snapshot");
    let expected_codes = vec![
        RoutingReasonCode::ExplicitSelection,
        RoutingReasonCode::PolicyDefault,
        RoutingReasonCode::CapabilityMatch,
        RoutingReasonCode::CostLimit,
        RoutingReasonCode::LatencyPreference,
        RoutingReasonCode::ResidencyConstraint,
        RoutingReasonCode::RunnerLocalRequired,
        RoutingReasonCode::FallbackAfterError,
        RoutingReasonCode::FallbackAfterCapacity,
        RoutingReasonCode::PolicyDenied,
    ];

    assert_eq!(capability.routing_reason_codes, expected_codes);

    let usage_routing = snapshot.usage.payload.routing.as_ref().unwrap();
    assert_eq!(usage_routing.selected_provider, "fake-hosted");
    assert_eq!(usage_routing.selected_model, "fake-deterministic-v1");
    assert_eq!(usage_routing.route_type, RouteType::Fallback);
    assert_eq!(
        usage_routing.reason_code,
        RoutingReasonCode::FallbackAfterError
    );
    assert_eq!(usage_routing.fallback_attempt, 1);
    assert_eq!(
        usage_routing.fallback_from_provider.as_deref(),
        Some("fake-hosted-primary")
    );

    let audit_routing = snapshot.audit.payload.routing.as_ref().unwrap();
    assert_eq!(audit_routing, usage_routing);
}

#[test]
fn gateway_simulation_eval_fixture_covers_provider_and_mcp_paths() {
    let fixture: GatewaySimulationFixture = fixture("gateway_simulation_eval");

    let report = fixture.validate().unwrap();

    assert!(report.routing_primary > 0);
    assert!(report.routing_fallback > 0);
    assert!(report.streaming_success > 0);
    assert!(report.non_streaming_success > 0);
    assert!(report.rate_limit > 0);
    assert!(report.malformed_stream > 0);
    assert!(report.partial_stream > 0);
    assert!(report.timeout > 0);
    assert!(report.cancellation > 0);
    assert!(report.policy_denial > 0);
    assert!(report.quota_denial > 0);
    assert!(report.mcp_lifecycle > 0);
}

#[test]
fn alpha_routing_reuse_fixture_names_are_stable() {
    let fixture: GatewaySimulationFixture = fixture("gateway_simulation_eval");
    let provider_cases: std::collections::BTreeMap<_, _> = fixture
        .provider_cases
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect();
    let mcp_cases: std::collections::BTreeMap<_, _> = fixture
        .mcp_cases
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect();

    let expected_provider_case_ids = [
        "provider_streaming_success_primary",
        "provider_non_streaming_success_fallback",
        "provider_rate_limit_error",
        "provider_malformed_stream_chunk",
        "provider_partial_stream",
        "provider_timeout",
        "provider_cancellation",
        "provider_policy_denial",
        "provider_quota_denial",
    ];
    for case_id in expected_provider_case_ids {
        assert!(
            provider_cases.contains_key(case_id),
            "missing reusable provider fixture case: {case_id}"
        );
    }

    let fallback_case = provider_cases["provider_non_streaming_success_fallback"];
    assert_eq!(fallback_case.route.primary_provider, "fake-hosted-primary");
    assert_eq!(
        fallback_case.route.fallback_provider.as_deref(),
        Some("fake-hosted-fallback")
    );
    assert_eq!(
        fallback_case.route.selected_provider,
        "fake-hosted-fallback"
    );
    assert!(fallback_case.route.fallback_used);
    assert_eq!(
        fallback_case.route.reason.as_deref(),
        Some("primary_rate_limited")
    );

    let primary_case = provider_cases["provider_streaming_success_primary"];
    assert_eq!(primary_case.route.selected_provider, "fake-hosted");
    assert!(!primary_case.route.fallback_used);

    assert!(mcp_cases.contains_key("mcp_gateway_hosted_lifecycle_tool_call"));
}
