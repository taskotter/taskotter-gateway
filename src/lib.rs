#![deny(missing_docs)]

//! TaskOtter Gateway foundation.
//!
//! The crate models the first stable boundaries for gateway requests,
//! capability declarations, policy checks, and usage events. It is intentionally
//! side-effect free so later service, MCP transport, and provider adapters can
//! plug into the same protocol contracts.

pub mod gateway;
pub mod policy;
pub mod protocol;
pub mod tool;
pub mod usage;

pub use gateway::{Gateway, GatewayError, GatewayOutcome};
pub use policy::{PolicyCheck, PolicyConstraint, PolicyDecision, PolicyEffect, PolicyEngine};
pub use protocol::{GatewayRequest, ProtocolError, ProtocolVersion};
pub use tool::{CapabilitySurface, ToolCall, ToolCapability};
pub use usage::{
    PrincipalKind, UsageEvent, UsageLifecycleStage, UsageMeter, UsagePrincipal, UsageSubject,
    UsageSubjectKind, UsageUnit, UsageUnitKind,
};
