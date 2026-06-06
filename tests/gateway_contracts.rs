use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use taskotter_gateway::{
    adapters::ProviderKind,
    app,
    contracts::{
        validate_gateway_protocol_version, ActorRef, ActorType, AuditEvent, AuditOutcome,
        GatewayHealth, GatewayRegistration, HealthStatus, McpHostingMode, NormalizedError,
        NormalizedErrorCode, PolicyInstruction, ProviderAdapterCapability, RuntimeFeatureFlags,
        ScopedCredentialRef, ScopedMcpSessionRequest, ScopedModelRequest, StreamFrame,
        StreamFrameType, UsageEvent, GATEWAY_PROTOCOL_VERSION,
    },
    fallback::{
        validate_fallback, FallbackAttemptKind, FallbackPolicyFailure, FallbackPolicyLimits,
        FallbackPolicyRequest, FallbackProviderCandidate, RequiredProviderCapability,
        RoutingReasonCode,
    },
    mcp::McpRuntimeHost,
    mcp::{
        evaluate_tool_call_policy, resolve_endpoint, McpEndpoint, McpHostMode,
        McpToolCredentialRequirement, McpToolNetworkReach, McpToolPolicyEffect,
        McpToolPolicyFixtureCase, McpToolRiskLevel, McpToolSideEffectProfile,
    },
    provider::{FakeProviderAdapter, ProviderAdapter},
    simulator::GatewaySimulationFixture,
    usage::UsageAuditEventV1,
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
            "agent_id": "agent_1",
            "workflow_id": "workflow_1"
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

fn bound_relay_payload() -> Value {
    json!({
        "request_id": "00000000-0000-0000-0000-000000000557",
        "subject": {
            "user_id": "user_1",
            "working_group_id": "wg_1",
            "agent_id": "agent_1",
            "workflow_id": "workflow_1",
            "data_boundary_id": "db_us_fixture"
        },
        "provider": {
            "provider_id": "provider_policy_bound",
            "kind": "open_ai_compatible",
            "model": "test-model",
            "endpoint_id": "endpoint_1",
            "allowed_working_group_ids": ["wg_1"],
            "allowed_agent_ids": ["agent_1"],
            "allowed_workflow_ids": ["workflow_1"],
            "allowed_models": ["test-model"],
            "data_boundary_id": "db_us_fixture"
        },
        "messages": [
            { "role": "user", "content": "hello" }
        ],
        "stream": true,
        "requested_max_tokens": 1024,
        "estimated_cost_micro_usd": 25_000
    })
}

fn fallback_candidate(
    provider_id: &str,
    provider_kind: ProviderKind,
    provider_family: &str,
) -> FallbackProviderCandidate {
    FallbackProviderCandidate {
        provider_id: provider_id.to_string(),
        provider_kind,
        provider_family: provider_family.to_string(),
        model: "test-model".to_string(),
        region: "us".to_string(),
        data_residency: "us".to_string(),
        credential_scope: "wg_1/provider/fallback/secret_ref_sensitive_provider_credential"
            .to_string(),
        supports_streaming: true,
        supports_json_output: true,
        supports_tool_calls: false,
    }
}

fn fallback_policy_request(
    attempt_kind: FallbackAttemptKind,
    primary: FallbackProviderCandidate,
    candidate: FallbackProviderCandidate,
) -> FallbackPolicyRequest {
    FallbackPolicyRequest {
        request_id: "req_fallback_policy".to_string(),
        correlation_id: "corr_fallback_policy".to_string(),
        working_group_id: "wg_1".to_string(),
        actor: ActorRef {
            actor_type: ActorType::Agent,
            id: "agent_1".to_string(),
        },
        policy_decision_id: "poldec_fallback_policy".to_string(),
        attempt_kind,
        routing_reason: RoutingReasonCode::PrimaryRateLimited,
        primary,
        candidate,
        required_capabilities: vec![
            RequiredProviderCapability::Streaming,
            RequiredProviderCapability::JsonOutput,
        ],
        original_limits: FallbackPolicyLimits {
            max_tokens: Some(8192),
            max_cost_micro_usd: Some(50_000),
        },
        candidate_limits: FallbackPolicyLimits {
            max_tokens: Some(8192),
            max_cost_micro_usd: Some(50_000),
        },
        allow_cross_provider: false,
        allow_runner_local: false,
    }
}

fn assert_denial_events_do_not_expose_values(
    failure: &FallbackPolicyFailure,
    prohibited_values: &[&str],
) {
    let usage = serde_json::to_string(&failure.usage_event).unwrap();
    let audit = serde_json::to_string(&failure.audit_event).unwrap();

    for prohibited in prohibited_values {
        assert!(
            !usage.contains(prohibited),
            "usage event exposed prohibited value: {prohibited}"
        );
        assert!(
            !audit.contains(prohibited),
            "audit event exposed prohibited value: {prohibited}"
        );
    }
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
async fn provider_policy_allows_only_bound_subject_model_and_data_boundary() {
    let (status, body) = post_json("/v1/ai/relay", bound_relay_payload()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["usage_audit_event"]["status"], "succeeded");
    assert_eq!(
        body["usage_audit_event"]["idempotency_key"],
        "usage:00000000-0000-0000-0000-000000000557"
    );

    let mut wrong_wg = bound_relay_payload();
    wrong_wg["subject"]["working_group_id"] = json!("wg_other");
    let (status, body) = post_json("/v1/ai/relay", wrong_wg).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["error"]["message"],
        "provider not available to working group"
    );
    assert_eq!(body["usage_audit_event"]["status"], "denied");

    let mut wrong_agent = bound_relay_payload();
    wrong_agent["subject"]["agent_id"] = json!("agent_other");
    let (status, body) = post_json("/v1/ai/relay", wrong_agent).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["message"], "provider not bound to agent");

    let mut wrong_workflow = bound_relay_payload();
    wrong_workflow["subject"]["workflow_id"] = json!("workflow_other");
    let (status, body) = post_json("/v1/ai/relay", wrong_workflow).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["message"], "provider not bound to workflow");

    let mut wrong_model = bound_relay_payload();
    wrong_model["provider"]["model"] = json!("not-allowed-model");
    let (status, body) = post_json("/v1/ai/relay", wrong_model).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["error"]["message"],
        "model not allowed for provider binding"
    );

    let mut wrong_boundary = bound_relay_payload();
    wrong_boundary["subject"]["data_boundary_id"] = json!("db_eu_fixture");
    let (status, body) = post_json("/v1/ai/relay", wrong_boundary).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["error"]["message"],
        "provider data boundary is not visible to subject"
    );
}

#[tokio::test]
async fn usage_cost_limits_are_enforced_before_and_during_provider_execution() {
    let mut preflight_token_denial = bound_relay_payload();
    preflight_token_denial["requested_max_tokens"] = json!(8_193);
    let (status, body) = post_json("/v1/ai/relay", preflight_token_denial).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["message"], "request exceeds token quota");
    assert_eq!(body["usage_audit_event"]["status"], "denied");

    let mut preflight_cost_denial = bound_relay_payload();
    preflight_cost_denial["estimated_cost_micro_usd"] = json!(50_001);
    let (status, body) = post_json("/v1/ai/relay", preflight_cost_denial).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["message"], "request exceeds cost quota");
    assert_eq!(body["usage_audit_event"]["status"], "denied");

    let mut mid_stream_quota = bound_relay_payload();
    mid_stream_quota["messages"][0]["content"] = json!("simulate:mid_stream_quota");
    let (status, body) = post_json("/v1/ai/relay", mid_stream_quota).await;
    assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
    assert_eq!(body["error"]["code"], "quota_exceeded");
    assert_eq!(body["error"]["retryable"], false);
    assert_eq!(body["usage_audit_event"]["status"], "quota_denied");
    assert_eq!(
        body["usage_audit_event"]["idempotency_key"],
        "usage:00000000-0000-0000-0000-000000000557"
    );
}

#[tokio::test]
async fn usage_audit_event_idempotency_key_is_stable_for_replayed_request() {
    let payload = bound_relay_payload();

    let (first_status, first_body) = post_json("/v1/ai/relay", payload.clone()).await;
    let (second_status, second_body) = post_json("/v1/ai/relay", payload).await;

    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(
        first_body["usage_audit_event"]["idempotency_key"],
        second_body["usage_audit_event"]["idempotency_key"]
    );
    assert_eq!(
        first_body["usage_audit_event"]["idempotency_key"],
        "usage:00000000-0000-0000-0000-000000000557"
    );
}

#[tokio::test]
async fn runtime_usage_audit_event_projects_to_control_plane_envelopes() {
    let mut payload = bound_relay_payload();
    payload["messages"][0]["content"] = json!("simulate:mid_stream_quota");
    let (status, body) = post_json("/v1/ai/relay", payload).await;

    assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
    let runtime_event: UsageAuditEventV1 =
        serde_json::from_value(body["usage_audit_event"].clone()).unwrap();
    let usage_event = runtime_event.to_usage_event();
    let audit_event = runtime_event.to_audit_event();

    let usage_value = serde_json::to_value(&usage_event).unwrap();
    let audit_value = serde_json::to_value(&audit_event).unwrap();
    let usage_round_trip: UsageEvent = serde_json::from_value(usage_value).unwrap();
    let audit_round_trip: AuditEvent = serde_json::from_value(audit_value).unwrap();

    assert_eq!(
        usage_round_trip.idempotency_key,
        runtime_event.idempotency_key
    );
    assert_eq!(
        usage_round_trip.source,
        taskotter_gateway::contracts::EventSource::Gateway
    );
    assert_eq!(
        usage_round_trip.policy_decision_id,
        runtime_event.decision_id
    );
    assert_eq!(
        audit_round_trip.policy_decision_id,
        runtime_event.decision_id
    );
    assert_eq!(
        audit_round_trip.payload.outcome,
        taskotter_gateway::contracts::AuditOutcome::Denied
    );
    assert_eq!(
        audit_round_trip.payload.feature_flag.as_deref(),
        Some("gateway.provider_routing.enabled")
    );
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
fn fallback_policy_allows_retry_and_same_provider_without_widening_scope() {
    let primary = fallback_candidate("provider_primary", ProviderKind::Hosted, "provider_family");
    let retry =
        fallback_policy_request(FallbackAttemptKind::Retry, primary.clone(), primary.clone());
    let same_provider = fallback_policy_request(
        FallbackAttemptKind::SameProvider,
        primary.clone(),
        fallback_candidate(
            "provider_secondary",
            ProviderKind::Hosted,
            "provider_family",
        ),
    );

    let retry_decision = validate_fallback(&retry).unwrap();
    let same_provider_decision = validate_fallback(&same_provider).unwrap();

    assert!(retry_decision.allowed);
    assert_eq!(retry_decision.selected_provider, "provider_primary");
    assert!(same_provider_decision.allowed);
    assert_eq!(
        same_provider_decision.selected_provider,
        "provider_secondary"
    );
}

#[test]
fn fallback_policy_requires_explicit_cross_provider_allowance() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let candidate = fallback_candidate("provider_b", ProviderKind::OpenAiCompatible, "provider_b");
    let denied = fallback_policy_request(
        FallbackAttemptKind::CrossProvider,
        primary.clone(),
        candidate.clone(),
    );
    let mut allowed = denied.clone();
    allowed.allow_cross_provider = true;

    let failure = validate_fallback(&denied).unwrap_err();
    let decision = validate_fallback(&allowed).unwrap();

    assert_eq!(failure.class, "cross_provider_fallback_not_allowed");
    assert_eq!(
        failure.normalized_error.code,
        NormalizedErrorCode::PolicyDenied
    );
    assert_eq!(
        failure.usage_event.policy_decision_id,
        "poldec_fallback_policy"
    );
    assert_eq!(failure.audit_event.payload.outcome, AuditOutcome::Denied);
    assert_eq!(
        failure.audit_event.payload.feature_flag.as_deref(),
        Some("gateway.provider_routing.enabled")
    );
    assert_denial_events_do_not_expose_values(
        &failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "secret_ref_sensitive_provider_credential",
        ],
    );
    assert!(decision.allowed);
    assert_eq!(decision.selected_provider, "provider_b");
}

#[test]
fn fallback_policy_allows_runner_local_only_with_explicit_scope() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let runner = fallback_candidate("runner_local", ProviderKind::LocalRunner, "runner_local");
    let mut denied = fallback_policy_request(
        FallbackAttemptKind::RunnerLocal,
        primary.clone(),
        runner.clone(),
    );
    denied.routing_reason = RoutingReasonCode::RunnerLocalRequired;
    let mut allowed = denied.clone();
    allowed.allow_runner_local = true;

    let failure = validate_fallback(&denied).unwrap_err();
    let decision = validate_fallback(&allowed).unwrap();

    assert_eq!(failure.class, "runner_local_fallback_not_allowed");
    assert_eq!(
        failure.normalized_error.code,
        NormalizedErrorCode::PolicyDenied
    );
    assert!(decision.allowed);
    assert_eq!(decision.selected_provider, "runner_local");
}

#[test]
fn fallback_policy_rejects_local_runner_candidate_in_same_provider_attempt() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let runner = fallback_candidate("runner_local", ProviderKind::LocalRunner, "provider_a");
    let denied = fallback_policy_request(
        FallbackAttemptKind::SameProvider,
        primary.clone(),
        runner.clone(),
    );

    let failure = validate_fallback(&denied).unwrap_err();

    assert_eq!(failure.class, "runner_local_fallback_not_allowed");
    assert_eq!(
        failure.normalized_error.code,
        NormalizedErrorCode::PolicyDenied
    );
    assert_denial_events_do_not_expose_values(
        &failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "secret_ref_sensitive_provider_credential",
        ],
    );
}

#[test]
fn fallback_policy_rejects_local_runner_candidate_in_retry_attempt() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let runner = fallback_candidate("provider_a", ProviderKind::LocalRunner, "provider_a");
    let denied =
        fallback_policy_request(FallbackAttemptKind::Retry, primary.clone(), runner.clone());

    let failure = validate_fallback(&denied).unwrap_err();

    assert_eq!(failure.class, "runner_local_fallback_not_allowed");
    assert_eq!(
        failure.normalized_error.code,
        NormalizedErrorCode::PolicyDenied
    );
    assert_denial_events_do_not_expose_values(
        &failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "secret_ref_sensitive_provider_credential",
        ],
    );
}

#[test]
fn fallback_policy_rejects_required_capability_drop_as_normalized_failure() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let mut candidate = fallback_candidate("provider_b", ProviderKind::Hosted, "provider_a");
    candidate.supports_json_output = false;
    let request = fallback_policy_request(FallbackAttemptKind::SameProvider, primary, candidate);

    let failure = validate_fallback(&request).unwrap_err();

    assert_eq!(failure.class, "required_capability_dropped");
    assert_eq!(
        failure.normalized_error.code,
        NormalizedErrorCode::PolicyDenied
    );
    assert!(!failure.normalized_error.retryable);
    assert_eq!(
        failure
            .usage_event
            .payload
            .measurements
            .runtime_capability
            .as_deref(),
        Some("gateway.fallback_denied.required_capability_dropped")
    );
    assert_denial_events_do_not_expose_values(
        &failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "secret_ref_sensitive_provider_credential",
        ],
    );
}

#[test]
fn fallback_policy_requires_candidate_to_directly_satisfy_required_capabilities() {
    let mut primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    primary.supports_json_output = false;
    let mut candidate = fallback_candidate("provider_b", ProviderKind::Hosted, "provider_a");
    candidate.supports_json_output = false;
    let request = fallback_policy_request(FallbackAttemptKind::SameProvider, primary, candidate);

    let failure = validate_fallback(&request).unwrap_err();

    assert_eq!(failure.class, "required_capability_dropped");
    assert_eq!(
        failure.normalized_error.code,
        NormalizedErrorCode::PolicyDenied
    );
    assert_denial_events_do_not_expose_values(
        &failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "secret_ref_sensitive_provider_credential",
        ],
    );
}

#[test]
fn fallback_policy_allows_required_capability_when_primary_metadata_is_missing_but_candidate_has_it(
) {
    let mut primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    primary.supports_json_output = false;
    let candidate = fallback_candidate("provider_b", ProviderKind::Hosted, "provider_a");
    let request = fallback_policy_request(FallbackAttemptKind::SameProvider, primary, candidate);

    let decision = validate_fallback(&request).unwrap();

    assert!(decision.allowed);
    assert_eq!(decision.selected_provider, "provider_b");
}

#[test]
fn fallback_policy_rejects_region_change_without_exposing_scope_values() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let mut widened_scope = fallback_candidate("provider_b", ProviderKind::Hosted, "provider_a");
    widened_scope.region = "eu".to_string();
    let scope_request = fallback_policy_request(
        FallbackAttemptKind::SameProvider,
        primary.clone(),
        widened_scope,
    );

    let scope_failure = validate_fallback(&scope_request).unwrap_err();

    assert_eq!(scope_failure.class, "fallback_scope_widened");
    assert_denial_events_do_not_expose_values(
        &scope_failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "eu",
            "secret_ref_sensitive_provider_credential",
        ],
    );
}

#[test]
fn fallback_policy_rejects_data_residency_change_without_exposing_raw_value() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let mut widened_scope = fallback_candidate("provider_b", ProviderKind::Hosted, "provider_a");
    widened_scope.data_residency = "restricted-eu-residency".to_string();
    let request = fallback_policy_request(
        FallbackAttemptKind::SameProvider,
        primary.clone(),
        widened_scope,
    );

    let failure = validate_fallback(&request).unwrap_err();

    assert_eq!(failure.class, "fallback_scope_widened");
    assert_denial_events_do_not_expose_values(
        &failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "restricted-eu-residency",
            "secret_ref_sensitive_provider_credential",
        ],
    );
}

#[test]
fn fallback_policy_rejects_credential_scope_change_without_exposing_raw_value() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let mut widened_scope = fallback_candidate("provider_b", ProviderKind::Hosted, "provider_a");
    widened_scope.credential_scope =
        "wg_1/provider/provider_b/secret_ref_sensitive_provider_credential".to_string();
    let request = fallback_policy_request(
        FallbackAttemptKind::SameProvider,
        primary.clone(),
        widened_scope,
    );

    let failure = validate_fallback(&request).unwrap_err();

    assert_eq!(failure.class, "fallback_scope_widened");
    assert_denial_events_do_not_expose_values(
        &failure,
        &[
            "wg_1/provider/provider_b",
            "secret_ref_sensitive_provider_credential",
        ],
    );
}

#[test]
fn fallback_policy_rejects_max_cost_limit_widening() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let mut widened_limits = fallback_policy_request(
        FallbackAttemptKind::SameProvider,
        primary.clone(),
        fallback_candidate("provider_c", ProviderKind::Hosted, "provider_a"),
    );
    widened_limits.candidate_limits.max_cost_micro_usd = Some(60_000);

    let limit_failure = validate_fallback(&widened_limits).unwrap_err();

    assert_eq!(limit_failure.class, "fallback_policy_limit_widened");
    assert_eq!(
        limit_failure
            .audit_event
            .payload
            .runtime_capability
            .as_deref(),
        Some("gateway.fallback_denied.fallback_policy_limit_widened")
    );
    assert_denial_events_do_not_expose_values(
        &limit_failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "secret_ref_sensitive_provider_credential",
        ],
    );
}

#[test]
fn fallback_policy_rejects_max_tokens_limit_widening() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let mut widened_limits = fallback_policy_request(
        FallbackAttemptKind::SameProvider,
        primary.clone(),
        fallback_candidate("provider_c", ProviderKind::Hosted, "provider_a"),
    );
    widened_limits.candidate_limits.max_tokens = Some(16_384);

    let failure = validate_fallback(&widened_limits).unwrap_err();

    assert_eq!(failure.class, "fallback_policy_limit_widened");
    assert_denial_events_do_not_expose_values(
        &failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "secret_ref_sensitive_provider_credential",
        ],
    );
}

#[test]
fn fallback_policy_rejects_candidate_limit_none_unlimited_path() {
    let primary = fallback_candidate("provider_a", ProviderKind::Hosted, "provider_a");
    let mut unlimited_limits = fallback_policy_request(
        FallbackAttemptKind::SameProvider,
        primary.clone(),
        fallback_candidate("provider_c", ProviderKind::Hosted, "provider_a"),
    );
    unlimited_limits.candidate_limits.max_tokens = None;
    unlimited_limits.candidate_limits.max_cost_micro_usd = None;

    let failure = validate_fallback(&unlimited_limits).unwrap_err();

    assert_eq!(failure.class, "fallback_policy_limit_widened");
    assert_denial_events_do_not_expose_values(
        &failure,
        &[
            "wg_1/provider/fallback/secret_ref_sensitive_provider_credential",
            "secret_ref_sensitive_provider_credential",
        ],
    );
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
fn mcp_tool_policy_fixtures_cover_risk_and_actor_bindings() {
    let cases: Vec<McpToolPolicyFixtureCase> = fixture("mcp_tool_policy_cases");

    assert_eq!(cases.len(), 14);
    assert!(cases
        .iter()
        .any(|case| case.request.tool.risk_level == McpToolRiskLevel::ReadOnly));
    assert!(cases.iter().any(|case| {
        case.request.tool.risk_level == McpToolRiskLevel::SensitiveRead
            && case.request.credential_ref.is_some()
    }));
    assert!(cases.iter().any(|case| {
        case.request.tool.risk_level == McpToolRiskLevel::Mutating
            && case.expected_effect == McpToolPolicyEffect::ApprovalRequired
    }));
    assert!(cases.iter().any(|case| {
        case.request.tool.risk_level == McpToolRiskLevel::Execution
            && case.expected_effect == McpToolPolicyEffect::Denied
    }));
    assert!(cases.iter().any(|case| {
        case.id == "sensitive_read_denies_optional_credential_scope_mismatch"
            && case.expected_reason_code.as_deref() == Some("credential_scope_mismatch")
    }));
    assert!(cases.iter().any(|case| {
        case.id == "sensitive_read_denies_optional_credential_reference_kind"
            && case.expected_reason_code.as_deref() == Some("credential_reference_kind_not_allowed")
    }));
    for expected_reason in [
        "working_group_scope_mismatch",
        "actor_type_not_allowed",
        "actor_id_not_allowed",
        "required_skill_missing",
        "agent_binding_not_allowed",
        "runner_binding_not_allowed",
        "credential_reference_required",
        "credential_scope_mismatch",
        "credential_reference_kind_not_allowed",
    ] {
        assert!(
            cases.iter().any(|case| {
                case.expected_effect == McpToolPolicyEffect::Denied
                    && case.expected_reason_code.as_deref() == Some(expected_reason)
            }),
            "missing MCP tool policy fixture for {expected_reason}"
        );
    }

    for case in cases {
        case.request.tool.validate_risk_metadata().unwrap();

        let outcome = evaluate_tool_call_policy(&case.request);

        assert_eq!(outcome.effect, case.expected_effect, "case {}", case.id);
        assert_eq!(
            outcome.reason_code, case.expected_reason_code,
            "case {}",
            case.id
        );
        assert_eq!(
            outcome.policy_decision_id,
            case.request.policy.policy_decision_id(),
            "case {}",
            case.id
        );
        assert_eq!(
            outcome.usage_event.payload.measurements.tool_invocations,
            Some(0),
            "case {} must remain pre-execution",
            case.id
        );
        assert_eq!(
            outcome.audit_event.payload.action, "gateway.mcp.tool.call",
            "case {}",
            case.id
        );
        assert_eq!(
            outcome.audit_event.resource.resource_type, "mcp_server",
            "case {}",
            case.id
        );
        assert_eq!(
            outcome.audit_event.policy_decision_id,
            case.request.policy.policy_decision_id(),
            "case {}",
            case.id
        );

        let serialized = serde_json::to_string(&outcome).unwrap();
        assert!(
            !serialized.contains("secret_ref_"),
            "case {} leaked credential reference in policy outcome",
            case.id
        );
        assert!(
            !serialized.contains("runner_unapproved"),
            "case {} leaked denied runner binding in policy outcome",
            case.id
        );
        assert!(
            !serialized.contains("wg_mcp_policy_evil"),
            "case {} leaked or accepted WG prefix-bypass scope in policy outcome",
            case.id
        );
    }
}

#[test]
fn mcp_tool_policy_rejects_invalid_risk_metadata_before_execution() {
    let mut cases: Vec<McpToolPolicyFixtureCase> = fixture("mcp_tool_policy_cases");
    let mut invalid = cases.remove(0).request;
    invalid.tool.risk_level = McpToolRiskLevel::ReadOnly;
    invalid.tool.side_effect_profile = McpToolSideEffectProfile::ExecutesCodeOrProcess;
    invalid.tool.network_reach = McpToolNetworkReach::ExternalInternet;
    invalid.tool.credential_requirement = McpToolCredentialRequirement::RequiredScopedReference;

    let outcome = evaluate_tool_call_policy(&invalid);

    assert_eq!(outcome.effect, McpToolPolicyEffect::Denied);
    assert_eq!(
        outcome.reason_code.as_deref(),
        Some("mcp_tool_risk_metadata_invalid")
    );
    assert_eq!(
        outcome.normalized_error.as_ref().unwrap().code,
        NormalizedErrorCode::PolicyDenied
    );
    assert_eq!(
        outcome.usage_event.payload.measurements.tool_invocations,
        Some(0)
    );
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
    assert!(matrix["gateway"]["fixtures"]
        .as_array()
        .expect("fixture paths must be declared")
        .iter()
        .any(|fixture| fixture == "fixtures/gateway/v0_1/mcp_tool_policy_cases.json"));
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
fn usage_cost_replay_fixture_deduplicates_retry_charges() {
    let fixture: GatewaySimulationFixture = fixture("gateway_simulation_eval");

    let report = fixture.validate().unwrap();

    assert_eq!(report.usage_replay_idempotency, 1);
    assert!(report.quota_denial > 0);
    assert!(report.policy_denial > 0);
    assert!(report.routing_fallback > 0);
}
