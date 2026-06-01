use crate::contracts::{
    AuditEvent, AuditOutcome, AuditPayload, EventActorRef, EventResourceRef, EventSource,
    GatewayCapabilityGate, HealthStatus, HighRiskGatewayCapability, McpHostCapability,
    McpHostHealth, McpHostingMode, NormalizedError, RuntimeFeatureFlags, ScopedMcpSessionRequest,
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
                routing: None,
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
                routing: None,
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
