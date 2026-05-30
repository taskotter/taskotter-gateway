use crate::contracts::{
    AuditEvent, AuditOutcome, AuditPayload, EventActorRef, EventResourceRef, EventSource,
    HealthStatus, McpHostCapability, McpHostHealth, McpHostingMode, NormalizedError,
    ScopedMcpSessionRequest, UsageEvent, UsageMeasurements, UsagePayload, UsageSubject,
    UsageSubjectType,
};

#[derive(Debug, Clone)]
pub struct McpRuntimeHost {
    host_id: String,
}

impl McpRuntimeHost {
    pub fn new(host_id: impl Into<String>) -> Self {
        Self {
            host_id: host_id.into(),
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
        Ok(McpSessionPlaceholder {
            session_id: request.session_id.clone(),
            host_id: self.host_id.clone(),
            hosting_mode: request.hosting_mode.clone(),
            allowed_tool_refs: request.allowed_tool_refs.clone(),
            lifecycle_state: "ready_placeholder".to_string(),
        })
    }

    pub fn usage_event(&self, request: &ScopedMcpSessionRequest) -> UsageEvent {
        let policy_decision_id = match &request.policy {
            crate::contracts::PolicyInstruction::DecisionRef { decision_ref, .. } => {
                decision_ref.clone()
            }
            crate::contracts::PolicyInstruction::SignedDispatchPlaceholder {
                instruction_ref,
                ..
            } => instruction_ref.clone(),
        };

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
                },
            },
        }
    }

    pub fn audit_event(&self, request: &ScopedMcpSessionRequest) -> AuditEvent {
        let policy_decision_id = match &request.policy {
            crate::contracts::PolicyInstruction::DecisionRef { decision_ref, .. } => {
                decision_ref.clone()
            }
            crate::contracts::PolicyInstruction::SignedDispatchPlaceholder {
                instruction_ref,
                ..
            } => instruction_ref.clone(),
        };

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
                outcome: AuditOutcome::Allowed,
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
