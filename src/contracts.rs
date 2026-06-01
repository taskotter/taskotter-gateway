use serde::{Deserialize, Serialize};

pub const GATEWAY_PROTOCOL_VERSION: &str = "gateway.v0.1";
pub const SUPPORTED_GATEWAY_PROTOCOL_VERSIONS: &[&str] = &[GATEWAY_PROTOCOL_VERSION];

pub fn validate_gateway_protocol_version(version: &str) -> Result<(), NormalizedError> {
    if SUPPORTED_GATEWAY_PROTOCOL_VERSIONS.contains(&version) {
        Ok(())
    } else {
        Err(NormalizedError {
            code: NormalizedErrorCode::InvalidGatewayRequest,
            message: format!("unsupported gateway protocol version: {version}"),
            retryable: false,
            upstream_status: None,
            provider_error_class: Some("unsupported_gateway_protocol_version".to_string()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    User,
    Agent,
    Workflow,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRef {
    pub actor_type: ActorType,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRegistration {
    pub protocol_version: String,
    pub gateway_id: String,
    pub instance_id: String,
    pub capabilities: Vec<String>,
    pub control_plane_relay_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayHealth {
    pub status: HealthStatus,
    pub gateway_id: String,
    pub provider_adapters: Vec<ProviderAdapterHealth>,
    pub mcp_hosts: Vec<McpHostHealth>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Ok,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAdapterHealth {
    pub provider: String,
    pub status: HealthStatus,
    pub supports_streaming: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpHostHealth {
    pub host_id: String,
    pub hosting_mode: McpHostingMode,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAdapterCapability {
    pub provider: String,
    pub adapter_version: String,
    pub supported_models: Vec<ModelCapability>,
    pub supports_streaming: bool,
    pub supports_tool_calls: bool,
    pub credential_ref_kinds: Vec<CredentialRefKind>,
    #[serde(default)]
    pub provider_family: Option<String>,
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub data_residency: Vec<String>,
    #[serde(default)]
    pub credential_scope_prefixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapability {
    pub model: String,
    pub context_window_tokens: u32,
    pub supports_json_output: bool,
    pub supports_tool_calls: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpHostCapability {
    pub host_id: String,
    pub supported_hosting_modes: Vec<McpHostingMode>,
    pub supports_lifecycle_placeholder: bool,
    pub high_risk_capabilities: Vec<GatewayCapabilityGate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpHostingMode {
    GatewayHosted,
    RunnerHosted,
    ExternalRemote,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFeatureFlags {
    #[serde(default)]
    pub hosted_mcp_billing_enabled: bool,
    #[serde(default)]
    pub provider_routing_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighRiskGatewayCapability {
    HostedMcpBilling,
    SensitiveProviderRouting,
}

impl HighRiskGatewayCapability {
    pub fn contract_name(self) -> &'static str {
        match self {
            Self::HostedMcpBilling => "gateway.hosted_mcp_billing",
            Self::SensitiveProviderRouting => "gateway.sensitive_provider_routing",
        }
    }

    pub fn feature_flag(self) -> &'static str {
        match self {
            Self::HostedMcpBilling => "gateway.hosted_mcp_billing.enabled",
            Self::SensitiveProviderRouting => "gateway.provider_routing.enabled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCapabilityGate {
    pub capability: HighRiskGatewayCapability,
    pub feature_flag: String,
    pub enabled: bool,
    pub default_policy_effect: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedModelRequest {
    pub protocol_version: String,
    pub request_id: String,
    pub correlation_id: String,
    pub working_group_id: String,
    pub actor: ActorRef,
    pub provider: String,
    pub model: String,
    pub messages: Vec<ModelMessage>,
    pub stream: bool,
    pub policy: PolicyInstruction,
    pub credential_ref: ScopedCredentialRef,
}

impl ScopedModelRequest {
    pub fn validate_boundary(&self) -> Result<(), NormalizedError> {
        self.policy.validate()?;
        self.credential_ref.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedMcpSessionRequest {
    pub protocol_version: String,
    pub session_id: String,
    pub correlation_id: String,
    pub working_group_id: String,
    pub actor: ActorRef,
    pub mcp_server_ref: String,
    pub hosting_mode: McpHostingMode,
    pub allowed_tool_refs: Vec<String>,
    pub policy: PolicyInstruction,
    pub credential_ref: Option<ScopedCredentialRef>,
}

impl ScopedMcpSessionRequest {
    pub fn validate_boundary(&self) -> Result<(), NormalizedError> {
        self.policy.validate()?;
        if let Some(credential_ref) = &self.credential_ref {
            credential_ref.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PolicyInstruction {
    DecisionRef {
        decision_ref: String,
        expires_at: String,
    },
    SignedDispatchPlaceholder {
        instruction_ref: String,
        signature_ref: String,
        policy_decision_id: String,
        expires_at: String,
    },
}

impl PolicyInstruction {
    pub fn policy_decision_id(&self) -> &str {
        match self {
            Self::DecisionRef { decision_ref, .. } => decision_ref,
            Self::SignedDispatchPlaceholder {
                policy_decision_id, ..
            } => policy_decision_id,
        }
    }

    pub fn validate(&self) -> Result<(), NormalizedError> {
        let valid = match self {
            Self::DecisionRef { decision_ref, .. } => decision_ref.starts_with("poldec_"),
            Self::SignedDispatchPlaceholder {
                instruction_ref,
                policy_decision_id,
                signature_ref,
                ..
            } => {
                instruction_ref.starts_with("gwi_")
                    && signature_ref.starts_with("sigref_")
                    && policy_decision_id.starts_with("poldec_")
            }
        };

        if valid {
            Ok(())
        } else {
            Err(NormalizedError::policy_denied(
                "policy_instruction_invalid",
                "Policy instruction reference is invalid.",
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedCredentialRef {
    pub kind: CredentialRefKind,
    pub reference: String,
    pub scope: String,
}

impl ScopedCredentialRef {
    pub fn validate(&self) -> Result<(), NormalizedError> {
        if self.reference.starts_with("secret_ref_") && !looks_like_raw_secret(&self.reference) {
            return Ok(());
        }

        Err(NormalizedError::policy_denied(
            "raw_credential_value_rejected",
            "Credential values must be provided as scoped secret references.",
        ))
    }
}

fn looks_like_raw_secret(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.contains("sk-")
        || lowered.contains("api_key")
        || lowered.contains("token=")
        || lowered.contains("bearer ")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialRefKind {
    SecretRef,
    RunnerJobCredentialRef,
    ExternalMcpCredentialRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelResponse {
    pub request_id: String,
    pub provider: String,
    pub model: String,
    pub content: String,
    pub finish_reason: FinishReason,
    pub usage: UsageMeasurement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCall,
    Refusal,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamFrame {
    pub request_id: String,
    pub sequence: u32,
    pub frame_type: StreamFrameType,
    pub delta: Option<String>,
    pub usage: Option<UsageMeasurement>,
    pub error: Option<NormalizedError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamFrameType {
    Start,
    ContentDelta,
    UsageDelta,
    Final,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageMeasurement {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub version: String,
    pub occurred_at: String,
    pub source: EventSource,
    pub working_group_id: String,
    pub actor: EventActorRef,
    pub resource: EventResourceRef,
    pub correlation_id: String,
    pub request_id: String,
    pub policy_decision_id: String,
    pub idempotency_key: String,
    pub payload: UsagePayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    ControlPlane,
    Runner,
    Gateway,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsagePayload {
    pub subject: UsageSubject,
    pub measurements: UsageMeasurements,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageSubject {
    #[serde(rename = "type")]
    pub subject_type: UsageSubjectType,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSubjectType {
    AgentRun,
    GatewayRequest,
    WorkflowRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageMeasurements {
    pub duration_ms: u64,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub tool_invocations: Option<u32>,
    pub estimated_cost_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metering_unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_capability: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub version: String,
    pub occurred_at: String,
    pub source: EventSource,
    pub working_group_id: String,
    pub actor: EventActorRef,
    pub resource: EventResourceRef,
    pub correlation_id: String,
    pub request_id: String,
    pub policy_decision_id: String,
    pub payload: AuditPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventActorRef {
    #[serde(rename = "type")]
    pub actor_type: EventActorType,
    pub id: String,
}

impl From<&ActorRef> for EventActorRef {
    fn from(actor: &ActorRef) -> Self {
        let actor_type = match actor.actor_type {
            ActorType::User => EventActorType::User,
            ActorType::Agent => EventActorType::Agent,
            ActorType::Workflow | ActorType::Service => EventActorType::Service,
        };

        Self {
            actor_type,
            id: actor.id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventActorType {
    User,
    Agent,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventResourceRef {
    #[serde(rename = "type")]
    pub resource_type: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditPayload {
    pub action: String,
    pub outcome: AuditOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_flag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Allowed,
    Denied,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedError {
    pub code: NormalizedErrorCode,
    pub message: String,
    pub retryable: bool,
    pub upstream_status: Option<u16>,
    pub provider_error_class: Option<String>,
}

impl NormalizedError {
    pub fn policy_denied(provider_error_class: &str, message: &str) -> Self {
        Self {
            code: NormalizedErrorCode::PolicyDenied,
            message: message.to_string(),
            retryable: false,
            upstream_status: None,
            provider_error_class: Some(provider_error_class.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedErrorCode {
    PolicyDenied,
    RateLimited,
    UpstreamUnavailable,
    UpstreamTimeout,
    MalformedUpstreamResponse,
    InvalidGatewayRequest,
}
