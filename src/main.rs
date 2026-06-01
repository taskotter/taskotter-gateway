use std::net::SocketAddr;

use taskotter_gateway::app;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let addr: SocketAddr = std::env::var("TASKOTTER_GATEWAY_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()
        .expect("TASKOTTER_GATEWAY_ADDR must be a socket address");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind gateway listener");

    tracing::info!(%addr, "taskotter gateway listening");
    axum::serve(listener, app())
        .await
        .expect("gateway server failed");
}
