//! Tool and capability declarations exposed through the gateway.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// High-level capability surfaces that can be routed through the gateway.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySurface {
    /// Model provider or model-compatible endpoint invocation.
    ModelProvider,
    /// MCP server or remote MCP endpoint.
    McpServer,
    /// Runner-hosted local tool capability.
    RunnerTool,
}

/// A capability that can be listed, authorized, and invoked.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolCapability {
    /// Stable capability identifier.
    pub id: String,
    /// Human-readable capability name.
    pub name: String,
    /// Capability surface used for routing and policy checks.
    pub surface: CapabilitySurface,
    /// Versioned schema reference, not an inline copied schema.
    pub input_schema_ref: String,
    /// Versioned schema reference, not an inline copied schema.
    pub output_schema_ref: String,
    /// Whether invocation may reach external networks.
    pub network_access: bool,
    /// Whether invocation requires secret material resolved by policy.
    pub requires_secret: bool,
}

/// A tool invocation payload before provider-specific transport translation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Capability identifier to invoke.
    pub capability_id: String,
    /// Versioned tool name or operation.
    pub operation: String,
    /// Structured arguments validated against `input_schema_ref`.
    pub arguments: Value,
}
