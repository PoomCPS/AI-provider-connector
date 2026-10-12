use axum::extract::State;
use axum::Json;
use serde::Serialize;
use ai_provider_connector_core::ProviderKind;
use crate::state::AppState;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ProviderStatus {
    pub id: String,
    pub kind: String,
    pub key_set: bool,
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
