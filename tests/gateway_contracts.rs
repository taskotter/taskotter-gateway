use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use taskotter_gateway::{
    adapters::ProviderKind,
    app,
    mcp::{resolve_endpoint, McpEndpoint, McpHostMode},
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
