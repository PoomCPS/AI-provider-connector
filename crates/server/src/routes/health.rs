use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub status: &'static str,
    pub providers: Vec<&'static str>,
}

/// Handler for GET /health
pub async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        providers: vec!["openai", "anthropic", "gemini"],
    })
}
