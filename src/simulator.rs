use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::contracts::{
    AuditEvent, EventSource, McpHostingMode, NormalizedError, NormalizedErrorCode,
    ScopedMcpSessionRequest, ScopedModelRequest, StreamFrame, StreamFrameType, UsageEvent,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewaySimulationFixture {
    pub fixture_version: String,
    pub seed: String,
    pub provider_cases: Vec<ProviderSimulationCase>,
    pub mcp_cases: Vec<McpSimulationCase>,
    #[serde(default)]
    pub usage_replay_cases: Vec<UsageReplayCase>,
}

impl GatewaySimulationFixture {
    pub fn validate(&self) -> Result<SimulationReport, SimulationError> {
        if self.seed.trim().is_empty() {
            return Err(SimulationError::new(
                "fixture_seed_empty",
                "simulation fixture must declare a deterministic seed",
            ));
        }
        if self.provider_cases.is_empty() {
            return Err(SimulationError::new(
                "provider_cases_empty",
                "simulation fixture must include provider cases",
            ));
        }
        if self.mcp_cases.is_empty() {
            return Err(SimulationError::new(
                "mcp_cases_empty",
                "simulation fixture must include MCP cases",
            ));
        }

        let mut report = SimulationReport::default();
        for case in &self.provider_cases {
            case.validate(&mut report)?;
        }
        for case in &self.mcp_cases {
            case.validate(&mut report)?;
        }
        for case in &self.usage_replay_cases {
            case.validate(&mut report)?;
        }

        report.validate_required_coverage()?;
        Ok(report)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSimulationCase {
    pub id: String,
    pub request: ScopedModelRequest,
    pub route: ProviderRouteSimulation,
    pub outcome: ProviderSimulationOutcome,
    pub usage_event: UsageEvent,
    pub audit_event: AuditEvent,
    #[serde(default)]
    pub stream_frames: Vec<StreamFrame>,
}

impl ProviderSimulationCase {
    fn validate(&self, report: &mut SimulationReport) -> Result<(), SimulationError> {
        self.request
            .validate_boundary()
            .map_err(|error| SimulationError::from_normalized(&self.id, error))?;
        self.route.validate(&self.id)?;
        validate_usage_lineage(&self.id, &self.usage_event, &self.request)?;
        validate_audit_lineage(&self.id, &self.audit_event, &self.request)?;

        match &self.outcome {
            ProviderSimulationOutcome::Success { streaming, .. } => {
                report.provider_success += 1;
                if *streaming {
                    validate_success_stream(&self.id, &self.stream_frames)?;
                    report.streaming_success += 1;
                } else if !self.stream_frames.is_empty() {
                    return Err(SimulationError::new(
                        "non_streaming_case_has_frames",
                        format!("{} declared non-streaming but includes frames", self.id),
                    ));
                } else {
                    report.non_streaming_success += 1;
                }
            }
            ProviderSimulationOutcome::ProviderError { error } => {
                if error.code == NormalizedErrorCode::RateLimited {
                    report.rate_limit += 1;
                } else {
                    report.provider_error += 1;
                }
                validate_terminal_error_frame(&self.id, &self.stream_frames, &error.code)?;
            }
            ProviderSimulationOutcome::MalformedChunk { sequence } => {
                ensure_frame(&self.id, &self.stream_frames, *sequence)?;
                report.malformed_stream += 1;
            }
            ProviderSimulationOutcome::PartialStream { delivered_frames } => {
                if self.stream_frames.len() != *delivered_frames as usize {
                    return Err(SimulationError::new(
                        "partial_stream_frame_count_mismatch",
                        format!("{} delivered frame count does not match fixture", self.id),
                    ));
                }
                if self
                    .stream_frames
                    .iter()
                    .any(|frame| frame.frame_type == StreamFrameType::Final)
                {
                    return Err(SimulationError::new(
                        "partial_stream_has_final_frame",
                        format!("{} partial stream must not include a final frame", self.id),
                    ));
                }
                report.partial_stream += 1;
            }
            ProviderSimulationOutcome::Timeout { timeout_ms } => {
                if *timeout_ms == 0 {
                    report.timeout += 1;
                } else {
                    return Err(SimulationError::new(
                        "timeout_fixture_not_deterministic",
                        format!("{} timeout fixture must use zero elapsed wait", self.id),
                    ));
                }
            }
            ProviderSimulationOutcome::Cancellation {
                cancelled_after_frames,
            } => {
                if self.stream_frames.len() < *cancelled_after_frames as usize {
                    return Err(SimulationError::new(
                        "cancellation_frame_count_mismatch",
                        format!(
                            "{} cancellation frame count does not match fixture",
                            self.id
                        ),
                    ));
                }
                report.cancellation += 1;
            }
            ProviderSimulationOutcome::PolicyDenial { decision_id, .. } => {
                validate_decision_id(&self.id, decision_id, &self.usage_event)?;
                report.policy_denial += 1;
            }
            ProviderSimulationOutcome::QuotaDenial {
                decision_id,
                expected_status,
                ..
            } => {
                validate_decision_id(&self.id, decision_id, &self.usage_event)?;
                if expected_status.as_deref() != Some("quota_denied") {
                    return Err(SimulationError::new(
                        "quota_denial_status_missing",
                        format!("{} quota denial must declare expected_status", self.id),
                    ));
                }
                report.quota_denial += 1;
            }
        }

        if self.route.fallback_used {
            report.routing_fallback += 1;
        } else {
            report.routing_primary += 1;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRouteSimulation {
    pub primary_provider: String,
    #[serde(default)]
    pub fallback_provider: Option<String>,
    pub selected_provider: String,
    pub fallback_used: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

impl ProviderRouteSimulation {
    fn validate(&self, case_id: &str) -> Result<(), SimulationError> {
        if self.fallback_used {
            let Some(fallback_provider) = &self.fallback_provider else {
                return Err(SimulationError::new(
                    "fallback_missing_provider",
                    format!("{case_id} marks fallback_used without fallback_provider"),
                ));
            };
            if self.selected_provider != *fallback_provider {
                return Err(SimulationError::new(
                    "fallback_selected_provider_mismatch",
                    format!("{case_id} selected provider must match fallback provider"),
                ));
            }
        } else if self.selected_provider != self.primary_provider {
            return Err(SimulationError::new(
                "primary_selected_provider_mismatch",
                format!("{case_id} selected provider must match primary provider"),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderSimulationOutcome {
    Success {
        streaming: bool,
        content: String,
    },
    ProviderError {
        error: NormalizedError,
    },
    MalformedChunk {
        sequence: u32,
    },
    PartialStream {
        delivered_frames: u32,
    },
    Timeout {
        timeout_ms: u64,
    },
    Cancellation {
        cancelled_after_frames: u32,
    },
    PolicyDenial {
        decision_id: String,
        reason: String,
    },
    QuotaDenial {
        decision_id: String,
        max_cost_micro_usd: u64,
        #[serde(default)]
        expected_status: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpSimulationCase {
    pub id: String,
    pub request: ScopedMcpSessionRequest,
    pub lifecycle_events: Vec<McpLifecycleEvent>,
    pub usage_event: UsageEvent,
    pub audit_event: AuditEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageReplayCase {
    pub id: String,
    pub request: ScopedModelRequest,
    pub usage_events: Vec<UsageEvent>,
    pub expected_unique_charge_count: u32,
}

impl UsageReplayCase {
    fn validate(&self, report: &mut SimulationReport) -> Result<(), SimulationError> {
        self.request
            .validate_boundary()
            .map_err(|error| SimulationError::from_normalized(&self.id, error))?;
        if self.usage_events.is_empty() {
            return Err(SimulationError::new(
                "usage_replay_events_empty",
                format!("{} must include retry/replay usage events", self.id),
            ));
        }

        let mut unique_charge_keys = HashSet::new();
        for event in &self.usage_events {
            validate_usage_lineage(&self.id, event, &self.request)?;
            if event.idempotency_key.trim().is_empty() {
                return Err(SimulationError::new(
                    "usage_idempotency_key_empty",
                    format!("{} usage event has an empty idempotency key", self.id),
                ));
            }
            unique_charge_keys.insert(event.idempotency_key.as_str());
        }

        if unique_charge_keys.len() != self.expected_unique_charge_count as usize {
            return Err(SimulationError::new(
                "usage_replay_unique_charge_mismatch",
                format!("{} replay would record duplicate usage charges", self.id),
            ));
        }

        if self.usage_events.len() > unique_charge_keys.len() {
            report.usage_replay_idempotency += 1;
        }
        Ok(())
    }
}

impl McpSimulationCase {
    fn validate(&self, report: &mut SimulationReport) -> Result<(), SimulationError> {
        self.request
            .validate_boundary()
            .map_err(|error| SimulationError::from_normalized(&self.id, error))?;
        validate_mcp_lineage(
            &self.id,
            &self.usage_event,
            &self.audit_event,
            &self.request,
        )?;

        let has_start = self
            .lifecycle_events
            .iter()
            .any(|event| event.phase == McpLifecyclePhase::Start);
        let has_health = self
            .lifecycle_events
            .iter()
            .any(|event| event.phase == McpLifecyclePhase::Health);
        let has_tool_call = self
            .lifecycle_events
            .iter()
            .any(|event| event.phase == McpLifecyclePhase::ToolCall);
        let has_stop = self
            .lifecycle_events
            .iter()
            .any(|event| event.phase == McpLifecyclePhase::Stop);

        if !(has_start && has_health && has_tool_call && has_stop) {
            return Err(SimulationError::new(
                "mcp_lifecycle_incomplete",
                format!(
                    "{} must include start/health/tool_call/stop events",
                    self.id
                ),
            ));
        }

        let tool_calls = self
            .lifecycle_events
            .iter()
            .filter(|event| event.phase == McpLifecyclePhase::ToolCall)
            .count() as u32;
        let recorded_tool_calls = self
            .usage_event
            .payload
            .measurements
            .tool_invocations
            .unwrap_or_default();
        if recorded_tool_calls < tool_calls {
            return Err(SimulationError::new(
                "mcp_usage_tool_invocations_missing",
                format!("{} usage event must record simulated tool calls", self.id),
            ));
        }

        if self.request.hosting_mode == McpHostingMode::GatewayHosted {
            report.mcp_gateway_hosted += 1;
        }
        report.mcp_lifecycle += 1;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpLifecycleEvent {
    pub sequence: u32,
    pub phase: McpLifecyclePhase,
    pub state: String,
    #[serde(default)]
    pub tool_ref: Option<String>,
    #[serde(default)]
    pub error: Option<NormalizedError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpLifecyclePhase {
    Start,
    Health,
    ToolCall,
    Error,
    Stop,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SimulationReport {
    pub provider_success: u32,
    pub streaming_success: u32,
    pub non_streaming_success: u32,
    pub routing_primary: u32,
    pub routing_fallback: u32,
    pub provider_error: u32,
    pub rate_limit: u32,
    pub malformed_stream: u32,
    pub partial_stream: u32,
    pub timeout: u32,
    pub cancellation: u32,
    pub policy_denial: u32,
    pub quota_denial: u32,
    pub usage_replay_idempotency: u32,
    pub mcp_lifecycle: u32,
    pub mcp_gateway_hosted: u32,
}

impl SimulationReport {
    fn validate_required_coverage(&self) -> Result<(), SimulationError> {
        let required = [
            ("provider_success", self.provider_success),
            ("streaming_success", self.streaming_success),
            ("non_streaming_success", self.non_streaming_success),
            ("routing_primary", self.routing_primary),
            ("routing_fallback", self.routing_fallback),
            ("rate_limit", self.rate_limit),
            ("malformed_stream", self.malformed_stream),
            ("partial_stream", self.partial_stream),
            ("timeout", self.timeout),
            ("cancellation", self.cancellation),
            ("policy_denial", self.policy_denial),
            ("quota_denial", self.quota_denial),
            ("mcp_lifecycle", self.mcp_lifecycle),
        ];

        for (name, count) in required {
            if count == 0 {
                return Err(SimulationError::new(
                    "simulation_coverage_missing",
                    format!("required coverage is missing: {name}"),
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationError {
    pub code: &'static str,
    pub message: String,
}

impl SimulationError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn from_normalized(case_id: &str, error: NormalizedError) -> Self {
        Self::new(
            "normalized_contract_error",
            format!("{case_id} failed contract validation: {}", error.message),
        )
    }
}

impl std::fmt::Display for SimulationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SimulationError {}

fn validate_usage_lineage(
    case_id: &str,
    usage: &UsageEvent,
    request: &ScopedModelRequest,
) -> Result<(), SimulationError> {
    if usage.source != EventSource::Gateway
        || usage.request_id != request.request_id
        || usage.correlation_id != request.correlation_id
        || usage.policy_decision_id != request.policy.policy_decision_id()
        || usage.policy_decision_id.starts_with("gwi_")
    {
        return Err(SimulationError::new(
            "usage_lineage_mismatch",
            format!("{case_id} usage event does not match request lineage"),
        ));
    }
    Ok(())
}

fn validate_audit_lineage(
    case_id: &str,
    audit: &AuditEvent,
    request: &ScopedModelRequest,
) -> Result<(), SimulationError> {
    if audit.source != EventSource::Gateway
        || audit.request_id != request.request_id
        || audit.correlation_id != request.correlation_id
        || audit.policy_decision_id != request.policy.policy_decision_id()
        || audit.policy_decision_id.starts_with("gwi_")
    {
        return Err(SimulationError::new(
            "audit_lineage_mismatch",
            format!("{case_id} audit event does not match request lineage"),
        ));
    }
    Ok(())
}

fn validate_mcp_lineage(
    case_id: &str,
    usage: &UsageEvent,
    audit: &AuditEvent,
    request: &ScopedMcpSessionRequest,
) -> Result<(), SimulationError> {
    if usage.request_id != request.session_id
        || audit.request_id != request.session_id
        || usage.correlation_id != request.correlation_id
        || audit.correlation_id != request.correlation_id
        || usage.policy_decision_id != request.policy.policy_decision_id()
        || audit.policy_decision_id != request.policy.policy_decision_id()
    {
        return Err(SimulationError::new(
            "mcp_lineage_mismatch",
            format!("{case_id} MCP usage/audit lineage does not match request"),
        ));
    }
    Ok(())
}

fn validate_decision_id(
    case_id: &str,
    decision_id: &str,
    usage: &UsageEvent,
) -> Result<(), SimulationError> {
    if decision_id != usage.policy_decision_id {
        return Err(SimulationError::new(
            "decision_id_mismatch",
            format!("{case_id} decision id does not match usage event"),
        ));
    }
    Ok(())
}

fn validate_success_stream(case_id: &str, frames: &[StreamFrame]) -> Result<(), SimulationError> {
    if frames.is_empty() {
        return Err(SimulationError::new(
            "streaming_case_missing_frames",
            format!("{case_id} declared streaming success without frames"),
        ));
    }
    if frames.first().map(|frame| &frame.frame_type) != Some(&StreamFrameType::Start)
        || frames.last().map(|frame| &frame.frame_type) != Some(&StreamFrameType::Final)
        || frames
            .last()
            .and_then(|frame| frame.usage.as_ref())
            .is_none()
    {
        return Err(SimulationError::new(
            "streaming_success_invalid",
            format!("{case_id} streaming success must start, finish, and include final usage"),
        ));
    }
    validate_frame_sequence(case_id, frames)
}

fn validate_terminal_error_frame(
    case_id: &str,
    frames: &[StreamFrame],
    expected_code: &NormalizedErrorCode,
) -> Result<(), SimulationError> {
    let Some(last) = frames.last() else {
        return Ok(());
    };
    if last.frame_type != StreamFrameType::Error
        || last.error.as_ref().map(|error| &error.code) != Some(expected_code)
    {
        return Err(SimulationError::new(
            "terminal_error_frame_mismatch",
            format!("{case_id} terminal stream error does not match outcome"),
        ));
    }
    validate_frame_sequence(case_id, frames)
}

fn ensure_frame(
    case_id: &str,
    frames: &[StreamFrame],
    sequence: u32,
) -> Result<(), SimulationError> {
    validate_frame_sequence(case_id, frames)?;
    if !frames.iter().any(|frame| frame.sequence == sequence) {
        return Err(SimulationError::new(
            "stream_frame_missing",
            format!("{case_id} does not include expected frame sequence {sequence}"),
        ));
    }
    Ok(())
}

fn validate_frame_sequence(case_id: &str, frames: &[StreamFrame]) -> Result<(), SimulationError> {
    for (expected, frame) in frames.iter().enumerate() {
        if frame.sequence != expected as u32 {
            return Err(SimulationError::new(
                "stream_sequence_gap",
                format!("{case_id} stream sequence is not contiguous"),
            ));
        }
    }
    Ok(())
}
