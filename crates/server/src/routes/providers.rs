use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ai_provider_connector_core::{ApiKey, ProviderKind};
use crate::state::AppState;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ProviderStatus {
    pub id: String,
    pub kind: String,
    pub key_set: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetKeyRequest {
    pub key: String,
}

pub fn parse_provider_id(id: &str) -> Option<(ProviderKind, &'static str)> {
    match id.to_lowercase().as_str() {
        "openai" => Some((ProviderKind::OpenAICompatible, "openai")),
        "anthropic" => Some((ProviderKind::Anthropic, "anthropic")),
        "gemini" => Some((ProviderKind::Gemini, "gemini")),
        _ => None,
    }
}

/// Handler for GET /v1/providers
pub async fn list_providers_handler(
    State(state): State<AppState>,
) -> Json<Vec<ProviderStatus>> {
    let providers = [
        ("openai", ProviderKind::OpenAICompatible, "OpenAICompatible"),
        ("anthropic", ProviderKind::Anthropic, "Anthropic"),
        ("gemini", ProviderKind::Gemini, "Gemini"),
    ];

    let mut list = Vec::new();
    for (id, kind, kind_name) in providers {
        let key_set = state
            .secret_store
            .get(kind)
            .ok()
            .flatten()
            .is_some();
        list.push(ProviderStatus {
            id: id.to_string(),
            kind: kind_name.to_string(),
            key_set,
        });
    }

    Json(list)
}

/// Handler for POST /v1/providers/{id}/key
pub async fn set_provider_key_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<SetKeyRequest>,
) -> Response {
    let (provider_kind, canonical_id) = match parse_provider_id(&id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "provider_not_found",
                    "provider": id,
                })),
            )
                .into_response();
        }
    };

    let api_key = match ApiKey::new(&payload.key) {
        Ok(k) => k,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_key",
                    "reason": err.to_string(),
                })),
            )
                .into_response();
        }
    };

    if let Err(e) = state.secret_store.set(provider_kind, api_key) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "secret_store_error",
                "message": e.to_string(),
            })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(json!({
            "provider": canonical_id,
            "key_set": true,
        })),
    )
        .into_response()
}

/// Handler for DELETE /v1/providers/{id}/key
pub async fn delete_provider_key_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let (provider_kind, canonical_id) = match parse_provider_id(&id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "provider_not_found",
                    "provider": id,
                })),
            )
                .into_response();
        }
    };

    let _ = state.secret_store.delete(provider_kind);

    (
        StatusCode::OK,
        Json(json!({
            "provider": canonical_id,
            "key_set": false,
        })),
    )
        .into_response()
}
