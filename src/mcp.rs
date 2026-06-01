use crate::contracts::{
    ActorRef, ActorType, AuditEvent, AuditOutcome, AuditPayload, CredentialRefKind, EventActorRef,
    EventResourceRef, EventSource, GatewayCapabilityGate, HealthStatus, HighRiskGatewayCapability,
    McpHostCapability, McpHostHealth, McpHostingMode, NormalizedError, NormalizedErrorCode,
    PolicyInstruction, RuntimeFeatureFlags, ScopedCredentialRef, ScopedMcpSessionRequest,
    UsageEvent, UsageMeasurements, UsagePayload, UsageSubject, UsageSubjectType,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpHostMode {
    StdioHosted,
    ExternalRemote,
    RunnerHosted,
    ManagedHosted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpEndpoint {
    pub integration_id: String,
    pub host_mode: McpHostMode,
    #[serde(default)]
    pub runner_id: Option<String>,
    #[serde(default)]
    pub remote_url: Option<String>,
    #[serde(default)]
    pub command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResolution {
    pub integration_id: String,
    pub host_mode: McpHostMode,
    pub lifecycle: String,
    pub requires_policy_decision: bool,
    pub starts_runtime: bool,
}

pub fn resolve_endpoint(endpoint: McpEndpoint) -> McpResolution {
    let lifecycle = match endpoint.host_mode {
        McpHostMode::StdioHosted => "backend_or_single_node_process",
        McpHostMode::ExternalRemote => "external_network_endpoint",
        McpHostMode::RunnerHosted => "runner_dispatched_process",
        McpHostMode::ManagedHosted => "managed_paid_runtime_placeholder",
    };

    McpResolution {
        integration_id: endpoint.integration_id,
        host_mode: endpoint.host_mode,
        lifecycle: lifecycle.to_string(),
        requires_policy_decision: true,
        starts_runtime: false,
    }
}

#[derive(Debug, Clone)]
pub struct McpRuntimeHost {
    host_id: String,
    feature_flags: RuntimeFeatureFlags,
}

impl McpRuntimeHost {
    pub fn new(host_id: impl Into<String>) -> Self {
        Self {
            host_id: host_id.into(),
            feature_flags: RuntimeFeatureFlags::default(),
        }
    }

    pub fn with_feature_flags(
        host_id: impl Into<String>,
        feature_flags: RuntimeFeatureFlags,
    ) -> Self {
        Self {
            host_id: host_id.into(),
            feature_flags,
        }
    }

    pub fn capability(&self) -> McpHostCapability {
        McpHostCapability {
            host_id: self.host_id.clone(),
            supported_hosting_modes: vec![
                McpHostingMode::GatewayHosted,
                McpHostingMode::RunnerHosted,
                McpHostingMode::ExternalRemote,
            ],
            supports_lifecycle_placeholder: true,
            high_risk_capabilities: vec![GatewayCapabilityGate {
                capability: HighRiskGatewayCapability::HostedMcpBilling,
                feature_flag: HighRiskGatewayCapability::HostedMcpBilling
                    .feature_flag()
                    .to_string(),
                enabled: self.feature_flags.hosted_mcp_billing_enabled,
                default_policy_effect: "deny".to_string(),
            }],
        }
    }

    pub fn health(&self, hosting_mode: McpHostingMode) -> McpHostHealth {
        McpHostHealth {
            host_id: self.host_id.clone(),
            hosting_mode,
            status: HealthStatus::Ok,
        }
    }

    pub fn open_session(
        &self,
        request: &ScopedMcpSessionRequest,
    ) -> Result<McpSessionPlaceholder, NormalizedError> {
        request.validate_boundary()?;
        if request.hosting_mode == McpHostingMode::GatewayHosted
            && !self.feature_flags.hosted_mcp_billing_enabled
        {
            return Err(NormalizedError::policy_denied(
                "feature_flag_disabled",
                "Hosted MCP paid runtime is disabled by default.",
            ));
        }

        Ok(McpSessionPlaceholder {
            session_id: request.session_id.clone(),
            host_id: self.host_id.clone(),
            hosting_mode: request.hosting_mode.clone(),
            allowed_tool_refs: request.allowed_tool_refs.clone(),
            lifecycle_state: "ready_placeholder".to_string(),
        })
    }

    pub fn usage_event(&self, request: &ScopedMcpSessionRequest) -> UsageEvent {
        let policy_decision_id = request.policy.policy_decision_id().to_string();

        UsageEvent {
            id: "evt_01J9Z4P4BS0M9P2QJ6T8Z6W2EP".to_string(),
            event_type: "usage.gateway_request.recorded".to_string(),
            version: "0.1.0".to_string(),
            occurred_at: "2026-01-01T00:00:01.000Z".to_string(),
            source: EventSource::Gateway,
            working_group_id: request.working_group_id.clone(),
            actor: EventActorRef::from(&request.actor),
            resource: EventResourceRef {
                resource_type: "mcp_server".to_string(),
                id: request.mcp_server_ref.clone(),
            },
            correlation_id: request.correlation_id.clone(),
            request_id: request.session_id.clone(),
            policy_decision_id,
            idempotency_key: format!("usage_{}", request.session_id),
            payload: UsagePayload {
                subject: UsageSubject {
                    subject_type: UsageSubjectType::GatewayRequest,
                    id: request.session_id.clone(),
                },
                measurements: UsageMeasurements {
                    duration_ms: 0,
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                    tool_invocations: Some(request.allowed_tool_refs.len() as u32),
                    estimated_cost_micros: Some(0),
                    metering_unit: Some("hosted_mcp_runtime_ms".to_string()),
                    runtime_capability: Some(
                        HighRiskGatewayCapability::HostedMcpBilling
                            .contract_name()
                            .to_string(),
                    ),
                },
            },
        }
    }

    pub fn audit_event(&self, request: &ScopedMcpSessionRequest) -> AuditEvent {
        let policy_decision_id = request.policy.policy_decision_id().to_string();

        AuditEvent {
            id: "evt_01J9Z4P4BS0M9P2QJ6T8Z6W2EQ".to_string(),
            event_type: "audit.policy_decision.denied".to_string(),
            version: "0.1.0".to_string(),
            occurred_at: "2026-01-01T00:00:02.000Z".to_string(),
            source: EventSource::Gateway,
            working_group_id: request.working_group_id.clone(),
            actor: EventActorRef::from(&request.actor),
            resource: EventResourceRef {
                resource_type: "mcp_server".to_string(),
                id: request.mcp_server_ref.clone(),
            },
            correlation_id: request.correlation_id.clone(),
            request_id: request.session_id.clone(),
            policy_decision_id,
            payload: AuditPayload {
                action: "gateway.mcp.session.open".to_string(),
                outcome: AuditOutcome::Denied,
                runtime_capability: Some(
                    HighRiskGatewayCapability::HostedMcpBilling
                        .contract_name()
                        .to_string(),
                ),
                feature_flag: Some(
                    HighRiskGatewayCapability::HostedMcpBilling
                        .feature_flag()
                        .to_string(),
                ),
                approval_ref: Some("policy_decision_ref".to_string()),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSessionPlaceholder {
    pub session_id: String,
    pub host_id: String,
    pub hosting_mode: McpHostingMode,
    pub allowed_tool_refs: Vec<String>,
    pub lifecycle_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolRiskLevel {
    ReadOnly,
    SensitiveRead,
    Mutating,
    Execution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolSideEffectProfile {
    None,
    ReadsSensitiveData,
    MutatesWorkspaceState,
    ExecutesCodeOrProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolCredentialRequirement {
    None,
    OptionalScopedReference,
    RequiredScopedReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolNetworkReach {
    None,
    GatewayInternal,
    RunnerNetwork,
    ExternalInternet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpToolPolicyMetadata {
    pub tool_ref: String,
    pub mcp_server_ref: String,
    pub owner: String,
    pub working_group_id: String,
    pub risk_level: McpToolRiskLevel,
    pub side_effect_profile: McpToolSideEffectProfile,
    pub credential_requirement: McpToolCredentialRequirement,
    pub network_reach: McpToolNetworkReach,
    pub allowed_actor_types: Vec<ActorType>,
    #[serde(default)]
    pub allowed_actor_ids: Vec<String>,
    #[serde(default)]
    pub required_skill_refs: Vec<String>,
    #[serde(default)]
    pub allowed_agent_ids: Vec<String>,
    #[serde(default)]
    pub allowed_runner_ids: Vec<String>,
    #[serde(default)]
    pub requires_approval: bool,
}

impl McpToolPolicyMetadata {
    pub fn validate_risk_metadata(&self) -> Result<(), NormalizedError> {
        if self.tool_ref.is_empty()
            || self.mcp_server_ref.is_empty()
            || self.owner.is_empty()
            || self.working_group_id.is_empty()
            || self.allowed_actor_types.is_empty()
        {
            return Err(NormalizedError::policy_denied(
                "mcp_tool_metadata_incomplete",
                "MCP tool policy metadata must declare owner, scope, and actor bounds.",
            ));
        }

        let valid = match self.risk_level {
            McpToolRiskLevel::ReadOnly => {
                self.side_effect_profile == McpToolSideEffectProfile::None
                    && !self.requires_approval
                    && self.network_reach != McpToolNetworkReach::ExternalInternet
            }
            McpToolRiskLevel::SensitiveRead => {
                self.side_effect_profile == McpToolSideEffectProfile::ReadsSensitiveData
                    && self.credential_requirement
                        != McpToolCredentialRequirement::RequiredScopedReference
            }
            McpToolRiskLevel::Mutating => {
                self.side_effect_profile == McpToolSideEffectProfile::MutatesWorkspaceState
                    && self.requires_approval
            }
            McpToolRiskLevel::Execution => {
                self.side_effect_profile == McpToolSideEffectProfile::ExecutesCodeOrProcess
                    && self.credential_requirement
                        == McpToolCredentialRequirement::RequiredScopedReference
                    && !self.allowed_runner_ids.is_empty()
                    && self.requires_approval
            }
        };

        if valid {
            Ok(())
        } else {
            Err(NormalizedError::policy_denied(
                "mcp_tool_risk_metadata_invalid",
                "MCP tool risk metadata is inconsistent with the declared side-effect profile.",
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpToolCallPolicyRequest {
    pub call_id: String,
    pub session_id: String,
    pub correlation_id: String,
    pub working_group_id: String,
    pub actor: ActorRef,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub skill_refs: Vec<String>,
    #[serde(default)]
    pub runner_id: Option<String>,
    pub tool: McpToolPolicyMetadata,
    pub policy: PolicyInstruction,
    #[serde(default)]
    pub credential_ref: Option<ScopedCredentialRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolPolicyEffect {
    Allowed,
    Denied,
    ApprovalRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpToolPolicyOutcome {
    pub effect: McpToolPolicyEffect,
    pub policy_decision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_error: Option<NormalizedError>,
    pub usage_event: UsageEvent,
    pub audit_event: AuditEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpToolPolicyFixtureCase {
    pub id: String,
    pub request: McpToolCallPolicyRequest,
    pub expected_effect: McpToolPolicyEffect,
    #[serde(default)]
    pub expected_reason_code: Option<String>,
}

pub fn evaluate_tool_call_policy(request: &McpToolCallPolicyRequest) -> McpToolPolicyOutcome {
    let decision_id = request.policy.policy_decision_id().to_string();

    if let Err(error) = request.policy.validate() {
        return policy_outcome(
            request,
            McpToolPolicyEffect::Denied,
            Some("policy_instruction_invalid"),
            Some(error),
            &decision_id,
        );
    }

    if let Err(error) = request.tool.validate_risk_metadata() {
        return policy_outcome(
            request,
            McpToolPolicyEffect::Denied,
            Some("mcp_tool_risk_metadata_invalid"),
            Some(error),
            &decision_id,
        );
    }

    if let Some(credential_ref) = &request.credential_ref {
        if let Err(error) = credential_ref.validate() {
            return policy_outcome(
                request,
                McpToolPolicyEffect::Denied,
                Some("raw_credential_value_rejected"),
                Some(error),
                &decision_id,
            );
        }
    }

    let denied_reason = tool_call_denial_reason(request);
    if let Some(reason) = denied_reason {
        return policy_outcome(
            request,
            McpToolPolicyEffect::Denied,
            Some(reason),
            Some(NormalizedError::policy_denied(
                reason,
                "MCP tool call is denied by gateway policy before execution.",
            )),
            &decision_id,
        );
    }

    if request.tool.requires_approval {
        return policy_outcome(
            request,
            McpToolPolicyEffect::ApprovalRequired,
            Some("approval_required_before_tool_execution"),
            Some(NormalizedError {
                code: NormalizedErrorCode::PolicyDenied,
                message: "MCP tool call requires approval before execution.".to_string(),
                retryable: false,
                upstream_status: None,
                provider_error_class: Some("approval_required_before_tool_execution".to_string()),
            }),
            &decision_id,
        );
    }

    policy_outcome(
        request,
        McpToolPolicyEffect::Allowed,
        None,
        None,
        &decision_id,
    )
}

fn tool_call_denial_reason(request: &McpToolCallPolicyRequest) -> Option<&'static str> {
    if request.working_group_id != request.tool.working_group_id {
        return Some("working_group_scope_mismatch");
    }

    if !request
        .tool
        .allowed_actor_types
        .iter()
        .any(|allowed| allowed == &request.actor.actor_type)
    {
        return Some("actor_type_not_allowed");
    }

    if !request.tool.allowed_actor_ids.is_empty()
        && !request
            .tool
            .allowed_actor_ids
            .iter()
            .any(|id| id == &request.actor.id)
    {
        return Some("actor_id_not_allowed");
    }

    if request
        .tool
        .required_skill_refs
        .iter()
        .any(|required| !request.skill_refs.iter().any(|skill| skill == required))
    {
        return Some("required_skill_missing");
    }

    if !request.tool.allowed_agent_ids.is_empty() {
        let actor_agent_id = match request.actor.actor_type {
            ActorType::Agent => Some(request.actor.id.as_str()),
            _ => None,
        };
        let request_agent_id = request.agent_id.as_deref().or(actor_agent_id);

        if !request_agent_id.is_some_and(|agent_id| {
            request
                .tool
                .allowed_agent_ids
                .iter()
                .any(|allowed| allowed == agent_id)
        }) {
            return Some("agent_binding_not_allowed");
        }
    }

    if !request.tool.allowed_runner_ids.is_empty()
        && !request.runner_id.as_deref().is_some_and(|runner_id| {
            request
                .tool
                .allowed_runner_ids
                .iter()
                .any(|allowed| allowed == runner_id)
        })
    {
        return Some("runner_binding_not_allowed");
    }

    match request.tool.credential_requirement {
        McpToolCredentialRequirement::None => {
            if request.credential_ref.is_some() {
                return Some("credential_reference_not_allowed");
            }
        }
        McpToolCredentialRequirement::OptionalScopedReference => {
            if let Some(credential_ref) = &request.credential_ref {
                return validate_scoped_credential_binding(
                    credential_ref,
                    &request.working_group_id,
                );
            }
        }
        McpToolCredentialRequirement::RequiredScopedReference => {
            let Some(credential_ref) = &request.credential_ref else {
                return Some("credential_reference_required");
            };
            if let Some(reason) =
                validate_scoped_credential_binding(credential_ref, &request.working_group_id)
            {
                return Some(reason);
            }
        }
    }

    None
}

fn validate_scoped_credential_binding(
    credential_ref: &ScopedCredentialRef,
    working_group_id: &str,
) -> Option<&'static str> {
    if !credential_scope_matches_working_group(&credential_ref.scope, working_group_id) {
        return Some("credential_scope_mismatch");
    }

    if !matches!(
        credential_ref.kind,
        CredentialRefKind::ExternalMcpCredentialRef | CredentialRefKind::RunnerJobCredentialRef
    ) {
        return Some("credential_reference_kind_not_allowed");
    }

    None
}

fn credential_scope_matches_working_group(scope: &str, working_group_id: &str) -> bool {
    scope == working_group_id
        || scope
            .strip_prefix(working_group_id)
            .is_some_and(|remainder| remainder.starts_with('/'))
}

fn policy_outcome(
    request: &McpToolCallPolicyRequest,
    effect: McpToolPolicyEffect,
    reason_code: Option<&'static str>,
    normalized_error: Option<NormalizedError>,
    policy_decision_id: &str,
) -> McpToolPolicyOutcome {
    let audit_outcome = match effect {
        McpToolPolicyEffect::Allowed => AuditOutcome::Allowed,
        McpToolPolicyEffect::Denied | McpToolPolicyEffect::ApprovalRequired => AuditOutcome::Denied,
    };

    let runtime_capability = HighRiskGatewayCapability::HostedMcpBilling
        .contract_name()
        .to_string();

    McpToolPolicyOutcome {
        effect,
        policy_decision_id: policy_decision_id.to_string(),
        reason_code: reason_code.map(str::to_string),
        normalized_error,
        usage_event: UsageEvent {
            id: format!("evt_usage_{}", request.call_id),
            event_type: "usage.gateway_request.recorded".to_string(),
            version: "0.1.0".to_string(),
            occurred_at: "2026-01-01T00:00:03.000Z".to_string(),
            source: EventSource::Gateway,
            working_group_id: request.working_group_id.clone(),
            actor: EventActorRef::from(&request.actor),
            resource: EventResourceRef {
                resource_type: "mcp_server".to_string(),
                id: request.tool.mcp_server_ref.clone(),
            },
            correlation_id: request.correlation_id.clone(),
            request_id: request.call_id.clone(),
            policy_decision_id: policy_decision_id.to_string(),
            idempotency_key: format!("usage_{}", request.call_id),
            payload: UsagePayload {
                subject: UsageSubject {
                    subject_type: UsageSubjectType::GatewayRequest,
                    id: request.call_id.clone(),
                },
                measurements: UsageMeasurements {
                    duration_ms: 0,
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                    tool_invocations: Some(0),
                    estimated_cost_micros: Some(0),
                    metering_unit: Some("mcp_tool_policy_check".to_string()),
                    runtime_capability: Some(runtime_capability.clone()),
                },
            },
        },
        audit_event: AuditEvent {
            id: format!("evt_audit_{}", request.call_id),
            event_type: format!(
                "audit.policy_decision.{}",
                audit_outcome_name(&audit_outcome)
            ),
            version: "0.1.0".to_string(),
            occurred_at: "2026-01-01T00:00:04.000Z".to_string(),
            source: EventSource::Gateway,
            working_group_id: request.working_group_id.clone(),
            actor: EventActorRef::from(&request.actor),
            resource: EventResourceRef {
                resource_type: "mcp_server".to_string(),
                id: request.tool.mcp_server_ref.clone(),
            },
            correlation_id: request.correlation_id.clone(),
            request_id: request.call_id.clone(),
            policy_decision_id: policy_decision_id.to_string(),
            payload: AuditPayload {
                action: "gateway.mcp.tool.call".to_string(),
                outcome: audit_outcome,
                runtime_capability: Some(runtime_capability),
                feature_flag: None,
                approval_ref: match reason_code {
                    Some("approval_required_before_tool_execution") => {
                        Some(policy_decision_id.to_string())
                    }
                    _ => None,
                },
            },
        },
    }
}

fn audit_outcome_name(outcome: &AuditOutcome) -> &'static str {
    match outcome {
        AuditOutcome::Allowed => "allowed",
        AuditOutcome::Denied => "denied",
        AuditOutcome::Succeeded => "succeeded",
        AuditOutcome::Failed => "failed",
    }
}
