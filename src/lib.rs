pub mod adapters;
pub mod api;
pub mod contracts;
pub mod mcp;
pub mod policy;
pub mod provider;
pub mod usage;

use std::sync::Arc;

use adapters::{ProviderRouter, StubProviderAdapter};
use api::{healthz, relay_ai_request, resolve_mcp_endpoint, GatewayState};
use axum::{
    routing::{get, post},
    Router,
};
use policy::StaticPolicyEngine;

pub fn app() -> Router {
    let router = ProviderRouter::new(Arc::new(StubProviderAdapter));
    let state = GatewayState {
        providers: Arc::new(router),
        policy: Arc::new(StaticPolicyEngine),
    };

    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/ai/relay", post(relay_ai_request))
        .route("/v1/mcp/resolve", post(resolve_mcp_endpoint))
        .with_state(state)
}

pub use contracts::*;
