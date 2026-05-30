//! Versioned protocol envelope for gateway traffic.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Current gateway protocol version for MVP contracts.
pub const CURRENT_PROTOCOL_VERSION: &str = "gateway.taskotter.dev/v1alpha1";

/// A strongly typed gateway protocol version.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProtocolVersion(String);

impl ProtocolVersion {
    /// Returns the current supported protocol version.
    #[must_use]
    pub fn current() -> Self {
        Self(CURRENT_PROTOCOL_VERSION.to_owned())
    }

    /// Returns the version string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validates the version against the current scaffold contract.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.0 == CURRENT_PROTOCOL_VERSION {
            Ok(())
        } else {
            Err(ProtocolError::UnsupportedVersion(self.0.clone()))
        }
    }
}

impl Default for ProtocolVersion {
    fn default() -> Self {
        Self::current()
    }
}

/// Top-level request envelope accepted by the gateway planner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GatewayRequest<T> {
    /// Protocol version for compatibility checks.
    pub version: ProtocolVersion,
    /// Stable request identifier supplied by the caller.
    pub request_id: String,
    /// Working Group or tenant boundary.
    pub working_group_id: String,
    /// User initiating the request.
    pub actor_id: String,
    /// Agent or automation flow acting on behalf of the user.
    pub principal_id: String,
    /// Request payload.
    pub payload: T,
}

impl<T> GatewayRequest<T> {
    /// Creates a request with the current protocol version.
    #[must_use]
    pub fn new(
        request_id: impl Into<String>,
        working_group_id: impl Into<String>,
        actor_id: impl Into<String>,
        principal_id: impl Into<String>,
        payload: T,
    ) -> Self {
        Self {
            version: ProtocolVersion::current(),
            request_id: request_id.into(),
            working_group_id: working_group_id.into(),
            actor_id: actor_id.into(),
            principal_id: principal_id.into(),
            payload,
        }
    }

    /// Validates invariant fields that every gateway request must carry.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.version.validate()?;
        require_non_empty("request_id", &self.request_id)?;
        require_non_empty("working_group_id", &self.working_group_id)?;
        require_non_empty("actor_id", &self.actor_id)?;
        require_non_empty("principal_id", &self.principal_id)
    }
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        Err(ProtocolError::MissingRequiredField(field))
    } else {
        Ok(())
    }
}

/// Protocol validation failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    /// The request uses a protocol version this gateway does not understand.
    UnsupportedVersion(String),
    /// The request omits a required envelope field.
    MissingRequiredField(&'static str),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported gateway protocol version: {version}")
            }
            Self::MissingRequiredField(field) => write!(f, "missing required field: {field}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_current_version() {
        let request = GatewayRequest::new("req_1", "wg_1", "usr_1", "agent_1", ());

        assert_eq!(request.validate(), Ok(()));
    }

    #[test]
    fn rejects_blank_identity_boundary() {
        let request = GatewayRequest::new("req_1", " ", "usr_1", "agent_1", ());

        assert_eq!(
            request.validate(),
            Err(ProtocolError::MissingRequiredField("working_group_id"))
        );
    }
}
