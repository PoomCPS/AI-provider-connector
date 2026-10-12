use std::sync::Arc;
use ai_provider_connector_core::SecretStore;

/// Shared application state for axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub secret_store: Arc<dyn SecretStore>,
}

impl AppState {
    pub fn new(secret_store: Arc<dyn SecretStore>) -> Self {
        Self { secret_store }
    }
}
