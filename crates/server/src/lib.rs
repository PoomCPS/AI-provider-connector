pub mod app;
pub mod routes;
pub mod state;

pub use app::build_app;
pub use state::AppState;

#[cfg(test)]
mod tests {
    use super::*;
    use ai_provider_connector_core::{
        AdapterError, ApiKey, CompletionRequest, CompletionResponse, FinishReason,
        InMemorySecretStore, ProviderAdapter, ProviderKind, SecretStore, StreamChunk,
    };
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use futures::stream::BoxStream;
    use http_body_util::BodyExt;
    use serde_json::Value;
    use std::sync::Arc;
    use tower::ServiceExt;

    struct FakeAdapter;

    #[async_trait::async_trait]
    impl ProviderAdapter for FakeAdapter {
        async fn complete(&self, _req: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
            Ok(CompletionResponse {
                content: "Hello from fake adapter".to_string(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                tool_calls: vec![],
            })
        }

        async fn stream(
            &self,
            _req: &CompletionRequest,
        ) -> Result<BoxStream<'static, Result<StreamChunk, AdapterError>>, AdapterError> {
            let chunk1 = StreamChunk {
                delta: "Hello ".to_string(),
                done: false,
            };
            let chunk2 = StreamChunk {
                delta: "world!".to_string(),
                done: true,
            };
            let s = futures::stream::iter(vec![Ok(chunk1), Ok(chunk2)]);
            Ok(Box::pin(s))
        }
    }

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

    #[tokio::test]
    async fn test_completions_no_key_forbidden() {
        let store = Arc::new(InMemorySecretStore::new());
        let state = AppState::new(store);
        let app = build_app(state);

        let req_body = serde_json::json!({
            "provider_id": "openai",
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "provider_key_not_configured");
        assert_eq!(json["provider"], "openai");
    }

    #[tokio::test]
    async fn test_completions_success() {
        let store = Arc::new(InMemorySecretStore::new());
        store
            .set(
                ProviderKind::OpenAICompatible,
                ApiKey::new("sk-test-valid-key-12345").unwrap(),
            )
            .unwrap();

        let state = AppState::new(store);
        state.set_adapter_override(ProviderKind::OpenAICompatible, Arc::new(FakeAdapter));
        let app = build_app(state);

        let req_body = serde_json::json!({
            "provider_id": "openai",
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["content"], "Hello from fake adapter");
        assert_eq!(json["input_tokens"], 10);
        assert_eq!(json["output_tokens"], 5);
    }

    #[tokio::test]
    async fn test_completions_stream_success() {
        let store = Arc::new(InMemorySecretStore::new());
        store
            .set(
                ProviderKind::Anthropic,
                ApiKey::new("sk-ant-valid-key-12345").unwrap(),
            )
            .unwrap();

        let state = AppState::new(store);
        state.set_adapter_override(ProviderKind::Anthropic, Arc::new(FakeAdapter));
        let app = build_app(state);

        let req_body = serde_json::json!({
            "provider_id": "anthropic",
            "model": "claude-3-5-sonnet-20241022",
            "stream": true,
            "messages": [{"role": "user", "content": "Hello streaming"}]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/completions/stream")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            content_type.contains("text/event-stream"),
            "expected text/event-stream, got {}",
            content_type
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        assert!(body_str.contains("data: {\"delta\":\"Hello \",\"done\":false}"));
        assert!(body_str.contains("data: [DONE]"));
    }
}
