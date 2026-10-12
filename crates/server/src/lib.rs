pub mod app;
pub mod routes;
pub mod state;

pub use app::build_app;
pub use state::AppState;

#[cfg(test)]
mod tests {
    use super::*;
    use ai_provider_connector_core::{ApiKey, InMemorySecretStore, ProviderKind, SecretStore};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let store = Arc::new(InMemorySecretStore::new());
        let state = AppState::new(store);
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        let providers = json["providers"].as_array().unwrap();
        assert_eq!(providers.len(), 3);
        assert_eq!(providers[0], "openai");
        assert_eq!(providers[1], "anthropic");
        assert_eq!(providers[2], "gemini");
    }

    #[tokio::test]
    async fn test_providers_endpoint() {
        let store = Arc::new(InMemorySecretStore::new());
        // Set a key for OpenAI
        store
            .set(
                ProviderKind::OpenAICompatible,
                ApiKey::new("sk-test-valid-key-12345").unwrap(),
            )
            .unwrap();

        let state = AppState::new(store);
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/providers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let list = json.as_array().expect("providers should be a JSON array");

        assert_eq!(list.len(), 3);

        let openai = list.iter().find(|p| p["id"] == "openai").unwrap();
        assert_eq!(openai["kind"], "OpenAICompatible");
        assert_eq!(openai["key_set"], true);
        assert!(openai.get("key").is_none()); // Key itself must never be exposed

        let anthropic = list.iter().find(|p| p["id"] == "anthropic").unwrap();
        assert_eq!(anthropic["kind"], "Anthropic");
        assert_eq!(anthropic["key_set"], false);

        let gemini = list.iter().find(|p| p["id"] == "gemini").unwrap();
        assert_eq!(gemini["kind"], "Gemini");
        assert_eq!(gemini["key_set"], false);
    }
}
