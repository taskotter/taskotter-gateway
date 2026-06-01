use serde::Serialize;

use crate::{
    adapters::ProviderKind,
    contracts::{
        EventSource, ModelResponse, NormalizedError, NormalizedErrorCode, ProviderCapabilityKind,
        RouteType, RoutingReasonCode, StreamFrame, StreamFrameType, UsageEvent,
    },
    fallback::FallbackPolicyFailure,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetricSample {
    pub name: &'static str,
    pub value: u64,
    pub unit: MetricUnit,
    pub labels: Vec<MetricLabel>,
}

impl MetricSample {
    pub fn label_value(&self, key: &str) -> Option<&'static str> {
        self.labels
            .iter()
            .find(|label| label.key == key)
            .map(|label| label.value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricUnit {
    Count,
    Milliseconds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MetricLabel {
    pub key: &'static str,
    pub value: &'static str,
}

pub const PROVIDER_LATENCY_MS: &str = "gateway_provider_latency_ms";
pub const STREAM_START_LATENCY_MS: &str = "gateway_stream_start_latency_ms";
pub const FALLBACK_COUNT: &str = "gateway_fallback_count";
pub const POLICY_DENIAL_COUNT: &str = "gateway_policy_denial_count";
pub const USAGE_EVENT_DELIVERY_LAG_MS: &str = "gateway_usage_event_delivery_lag_ms";
pub const PROVIDER_ERROR_COUNT: &str = "gateway_provider_error_count";

pub const METRIC_NAMES: [&str; 6] = [
    PROVIDER_LATENCY_MS,
    STREAM_START_LATENCY_MS,
    FALLBACK_COUNT,
    POLICY_DENIAL_COUNT,
    USAGE_EVENT_DELIVERY_LAG_MS,
    PROVIDER_ERROR_COUNT,
];

pub const BOUNDED_LABEL_KEYS: [&str; 8] = [
    "outcome",
    "provider_kind",
    "route_type",
    "reason_code",
    "error_code",
    "error_class",
    "event_source",
    "stream_frame",
];

pub fn provider_success_metrics(
    response: &ModelResponse,
    provider_kind: ProviderCapabilityKind,
    provider_latency_ms: u64,
) -> Vec<MetricSample> {
    let mut samples = vec![MetricSample {
        name: PROVIDER_LATENCY_MS,
        value: provider_latency_ms,
        unit: MetricUnit::Milliseconds,
        labels: vec![
            label("outcome", "success"),
            label(
                "provider_kind",
                provider_capability_kind_label(&provider_kind),
            ),
            label("route_type", route_type_label(&response.routing.route_type)),
            label(
                "reason_code",
                routing_reason_label(&response.routing.reason_code),
            ),
        ],
    }];

    if response.routing.route_type == RouteType::Fallback {
        samples.push(MetricSample {
            name: FALLBACK_COUNT,
            value: 1,
            unit: MetricUnit::Count,
            labels: vec![
                label("outcome", "fallback"),
                label(
                    "provider_kind",
                    provider_capability_kind_label(&provider_kind),
                ),
                label(
                    "reason_code",
                    routing_reason_label(&response.routing.reason_code),
                ),
            ],
        });
    }

    samples
}

pub fn stream_start_latency_metric(
    first_frame: &StreamFrame,
    provider_kind: ProviderCapabilityKind,
    stream_start_latency_ms: u64,
) -> MetricSample {
    MetricSample {
        name: STREAM_START_LATENCY_MS,
        value: stream_start_latency_ms,
        unit: MetricUnit::Milliseconds,
        labels: vec![
            label("outcome", "success"),
            label(
                "provider_kind",
                provider_capability_kind_label(&provider_kind),
            ),
            label("stream_frame", stream_frame_label(&first_frame.frame_type)),
            label(
                "route_type",
                first_frame
                    .routing
                    .as_ref()
                    .map(|routing| route_type_label(&routing.route_type))
                    .unwrap_or("unknown"),
            ),
        ],
    }
}

pub fn fallback_denial_metrics(failure: &FallbackPolicyFailure) -> Vec<MetricSample> {
    vec![MetricSample {
        name: POLICY_DENIAL_COUNT,
        value: 1,
        unit: MetricUnit::Count,
        labels: vec![
            label("outcome", "denial"),
            label("provider_kind", "unknown"),
            label("route_type", "denied"),
            label("error_class", sanitized_error_class(Some(failure.class))),
        ],
    }]
}

pub fn usage_event_delivery_lag_metric(
    usage_event: &UsageEvent,
    delivery_lag_ms: u64,
) -> MetricSample {
    MetricSample {
        name: USAGE_EVENT_DELIVERY_LAG_MS,
        value: delivery_lag_ms,
        unit: MetricUnit::Milliseconds,
        labels: vec![
            label("outcome", "success"),
            label("event_source", event_source_label(&usage_event.source)),
        ],
    }
}

pub fn provider_error_metric(error: &NormalizedError, provider_kind: ProviderKind) -> MetricSample {
    MetricSample {
        name: PROVIDER_ERROR_COUNT,
        value: 1,
        unit: MetricUnit::Count,
        labels: vec![
            label("outcome", "error"),
            label("provider_kind", provider_kind_label(provider_kind)),
            label("error_code", normalized_error_code_label(&error.code)),
            label(
                "error_class",
                sanitized_error_class(error.provider_error_class.as_deref()),
            ),
        ],
    }
}

pub fn metric_labels_are_bounded(sample: &MetricSample) -> bool {
    METRIC_NAMES.contains(&sample.name)
        && sample
            .labels
            .iter()
            .all(|label| BOUNDED_LABEL_KEYS.contains(&label.key) && label_value_is_bounded(label))
}

fn label(key: &'static str, value: &'static str) -> MetricLabel {
    MetricLabel { key, value }
}

fn label_value_is_bounded(label: &MetricLabel) -> bool {
    match label.key {
        "outcome" => matches!(label.value, "success" | "fallback" | "denial" | "error"),
        "provider_kind" => matches!(
            label.value,
            "hosted" | "open_ai_compatible" | "local_runner" | "future_adapter" | "unknown"
        ),
        "route_type" => matches!(label.value, "primary" | "fallback" | "denied" | "unknown"),
        "reason_code" => matches!(
            label.value,
            "explicit_selection"
                | "policy_default"
                | "capability_match"
                | "cost_limit"
                | "latency_preference"
                | "residency_constraint"
                | "runner_local_required"
                | "fallback_after_error"
                | "fallback_after_capacity"
                | "policy_denied"
        ),
        "error_code" => matches!(
            label.value,
            "policy_denied"
                | "rate_limited"
                | "upstream_unavailable"
                | "upstream_timeout"
                | "malformed_upstream_response"
                | "invalid_gateway_request"
        ),
        "error_class" => matches!(
            label.value,
            "rate_limited"
                | "quota_exhausted"
                | "invalid_request"
                | "authentication_failed"
                | "permission_denied"
                | "upstream_error"
                | "upstream_timeout"
                | "unsuccessful_response"
                | "openai_chat_response_malformed"
                | "openai_stream_missing_finish_reason"
                | "fake_rate_limit"
                | "provider_capability_disabled"
                | "provider_capability_mismatch"
                | "provider_streaming_not_supported"
                | "provider_credential_ref_kind_not_supported"
                | "provider_model_not_supported"
                | "fallback_reason_not_retryable"
                | "runner_local_fallback_not_allowed"
                | "retry_target_changed"
                | "same_provider_family_changed"
                | "cross_provider_fallback_not_allowed"
                | "required_capability_dropped"
                | "fallback_scope_widened"
                | "fallback_policy_limit_widened"
                | "other"
        ),
        "event_source" => matches!(label.value, "control_plane" | "runner" | "gateway"),
        "stream_frame" => matches!(
            label.value,
            "start" | "content_delta" | "usage_delta" | "final" | "error"
        ),
        _ => false,
    }
}

fn provider_capability_kind_label(kind: &ProviderCapabilityKind) -> &'static str {
    match kind {
        ProviderCapabilityKind::Hosted => "hosted",
        ProviderCapabilityKind::OpenAiCompatible => "open_ai_compatible",
        ProviderCapabilityKind::LocalRunner => "local_runner",
        ProviderCapabilityKind::FutureAdapter => "future_adapter",
    }
}

fn provider_kind_label(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Hosted => "hosted",
        ProviderKind::OpenAiCompatible => "open_ai_compatible",
        ProviderKind::LocalRunner => "local_runner",
        ProviderKind::FutureAdapter => "future_adapter",
    }
}

fn route_type_label(route_type: &RouteType) -> &'static str {
    match route_type {
        RouteType::Primary => "primary",
        RouteType::Fallback => "fallback",
        RouteType::Denied => "denied",
    }
}

fn routing_reason_label(reason: &RoutingReasonCode) -> &'static str {
    match reason {
        RoutingReasonCode::ExplicitSelection => "explicit_selection",
        RoutingReasonCode::PolicyDefault => "policy_default",
        RoutingReasonCode::CapabilityMatch => "capability_match",
        RoutingReasonCode::CostLimit => "cost_limit",
        RoutingReasonCode::LatencyPreference => "latency_preference",
        RoutingReasonCode::ResidencyConstraint => "residency_constraint",
        RoutingReasonCode::RunnerLocalRequired => "runner_local_required",
        RoutingReasonCode::FallbackAfterError => "fallback_after_error",
        RoutingReasonCode::FallbackAfterCapacity => "fallback_after_capacity",
        RoutingReasonCode::PolicyDenied => "policy_denied",
    }
}

fn normalized_error_code_label(code: &NormalizedErrorCode) -> &'static str {
    match code {
        NormalizedErrorCode::PolicyDenied => "policy_denied",
        NormalizedErrorCode::RateLimited => "rate_limited",
        NormalizedErrorCode::UpstreamUnavailable => "upstream_unavailable",
        NormalizedErrorCode::UpstreamTimeout => "upstream_timeout",
        NormalizedErrorCode::MalformedUpstreamResponse => "malformed_upstream_response",
        NormalizedErrorCode::InvalidGatewayRequest => "invalid_gateway_request",
    }
}

fn sanitized_error_class(class: Option<&str>) -> &'static str {
    match class {
        Some("rate_limited") => "rate_limited",
        Some("quota_exhausted") => "quota_exhausted",
        Some("invalid_request") => "invalid_request",
        Some("authentication_failed") => "authentication_failed",
        Some("permission_denied") => "permission_denied",
        Some("upstream_error") => "upstream_error",
        Some("upstream_timeout") => "upstream_timeout",
        Some("unsuccessful_response") => "unsuccessful_response",
        Some("openai_chat_response_malformed") => "openai_chat_response_malformed",
        Some("openai_stream_missing_finish_reason") => "openai_stream_missing_finish_reason",
        Some("fake_rate_limit") => "fake_rate_limit",
        Some("provider_capability_disabled") => "provider_capability_disabled",
        Some("provider_capability_mismatch") => "provider_capability_mismatch",
        Some("provider_streaming_not_supported") => "provider_streaming_not_supported",
        Some("provider_credential_ref_kind_not_supported") => {
            "provider_credential_ref_kind_not_supported"
        }
        Some("provider_model_not_supported") => "provider_model_not_supported",
        Some("fallback_reason_not_retryable") => "fallback_reason_not_retryable",
        Some("runner_local_fallback_not_allowed") => "runner_local_fallback_not_allowed",
        Some("retry_target_changed") => "retry_target_changed",
        Some("same_provider_family_changed") => "same_provider_family_changed",
        Some("cross_provider_fallback_not_allowed") => "cross_provider_fallback_not_allowed",
        Some("required_capability_dropped") => "required_capability_dropped",
        Some("fallback_scope_widened") => "fallback_scope_widened",
        Some("fallback_policy_limit_widened") => "fallback_policy_limit_widened",
        _ => "other",
    }
}

fn event_source_label(source: &EventSource) -> &'static str {
    match source {
        EventSource::ControlPlane => "control_plane",
        EventSource::Runner => "runner",
        EventSource::Gateway => "gateway",
    }
}

fn stream_frame_label(frame_type: &StreamFrameType) -> &'static str {
    match frame_type {
        StreamFrameType::Start => "start",
        StreamFrameType::ContentDelta => "content_delta",
        StreamFrameType::UsageDelta => "usage_delta",
        StreamFrameType::Final => "final",
        StreamFrameType::Error => "error",
    }
}
