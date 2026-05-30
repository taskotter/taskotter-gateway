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
