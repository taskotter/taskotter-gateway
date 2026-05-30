use crate::contracts::{
    AuditEvent, AuditOutcome, EventActorRef, EventResourceRef, EventSource, HealthStatus,
    McpHostCapability, McpHostHealth, McpHostingMode, NormalizedError, ScopedMcpSessionRequest,
    UsageEvent, UsageMeasurements, UsageSubject, UsageSubjectType,
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
                Some(decision_ref.clone())
            }
            crate::contracts::PolicyInstruction::SignedDispatchPlaceholder { .. } => None,
        };

        UsageEvent {
            schema_version: "usage-event@0.1.0".to_string(),
            event_id: "usevt_01J9Z4P4BS0M9P2QJ6T8Z6W2EP".to_string(),
            working_group_id: request.working_group_id.clone(),
            source: EventSource::Gateway,
            occurred_at: "2026-01-01T00:00:01.000Z".to_string(),
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
            policy_decision_id,
            idempotency_key: Some(format!("usage_{}", request.session_id)),
        }
    }

    pub fn audit_event(&self, request: &ScopedMcpSessionRequest) -> AuditEvent {
        let policy_decision_id = match &request.policy {
            crate::contracts::PolicyInstruction::DecisionRef { decision_ref, .. } => {
                Some(decision_ref.clone())
            }
            crate::contracts::PolicyInstruction::SignedDispatchPlaceholder { .. } => None,
        };

        AuditEvent {
            schema_version: "audit-event@0.1.0".to_string(),
            event_id: "aud_01J9Z4P4BS0M9P2QJ6T8Z6W2EP".to_string(),
            working_group_id: request.working_group_id.clone(),
            actor: EventActorRef::from(&request.actor),
            action: "gateway.mcp.session.open".to_string(),
            resource: EventResourceRef {
                resource_type: "mcp_server".to_string(),
                id: request.mcp_server_ref.clone(),
            },
            outcome: AuditOutcome::Allowed,
            occurred_at: "2026-01-01T00:00:02.000Z".to_string(),
            request_id: Some(request.session_id.clone()),
            policy_decision_id,
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
