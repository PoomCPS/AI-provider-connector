use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use crate::routes::{
    completions::{completions_handler, completions_stream_handler},
    dashboard::dashboard_handler,
    health::health_handler,
    providers::{delete_provider_key_handler, list_providers_handler, set_provider_key_handler},
};
use crate::state::AppState;

/// Constructs the primary axum Router for the application.
pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/dashboard", get(dashboard_handler))
        .route("/health", get(health_handler))
        .route("/v1/providers", get(list_providers_handler))
        .route(
            "/v1/providers/{id}/key",
            post(set_provider_key_handler).delete(delete_provider_key_handler),
        )
        .route("/v1/completions", post(completions_handler))
        .route("/v1/completions/stream", post(completions_stream_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
