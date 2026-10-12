//! Server binary for AI Provider Connector.

use std::net::SocketAddr;
use std::sync::Arc;
use ai_provider_connector_core::KeyringSecretStore;
use ai_provider_connector_server::{build_app, AppState};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ai_provider_connector_server=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let host = std::env::var("AI_CONNECTOR_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("AI_CONNECTOR_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6173);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;

    let secret_store = Arc::new(KeyringSecretStore::new());
    let state = AppState::new(secret_store);
    let app = build_app(state);

    info!("Starting AI Provider Connector server on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
