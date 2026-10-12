//! OpenAI-compatible adapter implementation.

pub mod wire;

use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::BoxStream;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use crate::adapter::{AdapterError, ProviderAdapter};
use crate::protocol::{CompletionRequest, CompletionResponse, StreamChunk};
use crate::secret_store::SecretStore;
use crate::types::{ApiKey, EndpointUrl, ProviderKind};
use wire::{map_openai_http_error, parse_openai_sse_data, OpenAIChatRequest, OpenAIChatResponse};

/// Adapter communicating with OpenAI or any OpenAI-compatible endpoint
/// (Ollama, DeepSeek, Groq, OpenRouter, LM Studio, vLLM).
pub struct OpenAIAdapter {
    client: reqwest::Client,
    endpoint: EndpointUrl,
    secret_store: Arc<dyn SecretStore>,
}

impl OpenAIAdapter {
    pub fn new(endpoint: EndpointUrl, secret_store: Arc<dyn SecretStore>) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint,
            secret_store,
        }
    }

    pub fn with_client(
        client: reqwest::Client,
        endpoint: EndpointUrl,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            client,
            endpoint,
            secret_store,
        }
    }

    fn get_api_key(&self) -> Result<ApiKey, AdapterError> {
        self.secret_store
            .get(ProviderKind::OpenAICompatible)
            .map_err(|e| AdapterError::AuthenticationFailed(format!("failed to fetch key from store: {e}")))?
            .ok_or_else(|| AdapterError::AuthenticationFailed("no API key set for OpenAI".into()))
    }

    fn build_url(&self) -> String {
        let base = self.endpoint.as_str().trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{}/chat/completions", base)
        }
    }

    fn extract_retry_after(headers: &HeaderMap) -> Option<Duration> {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
    }
}

#[async_trait]
impl ProviderAdapter for OpenAIAdapter {
    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
        let api_key = self.get_api_key()?;
        let url = self.build_url();
        let payload = OpenAIChatRequest::from_normalized(request);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let auth_val = format!("Bearer {}", api_key.expose_secret());
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_val)
                .map_err(|e| AdapterError::AuthenticationFailed(format!("invalid header value: {e}")))?,
        );

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        let status = response.status().as_u16();
        let retry_after = Self::extract_retry_after(response.headers());

        if status != 200 {
            let body = response.text().await.unwrap_or_default();
            return Err(map_openai_http_error(status, &body, retry_after));
        }

        let body: OpenAIChatResponse = response
            .json()
            .await
            .map_err(|e| AdapterError::Serialization(e.to_string()))?;

        body.into_normalized()
    }

    async fn stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<BoxStream<'static, Result<StreamChunk, AdapterError>>, AdapterError> {
        let api_key = self.get_api_key()?;
        let url = self.build_url();
        let mut payload = OpenAIChatRequest::from_normalized(request);
        payload.stream = true;

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let auth_val = format!("Bearer {}", api_key.expose_secret());
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_val)
                .map_err(|e| AdapterError::AuthenticationFailed(format!("invalid header value: {e}")))?,
        );

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        let status = response.status().as_u16();
        let retry_after = Self::extract_retry_after(response.headers());

        if status != 200 {
            let body = response.text().await.unwrap_or_default();
            return Err(map_openai_http_error(status, &body, retry_after));
        }

        let byte_stream = response.bytes_stream().eventsource();
        let chunk_stream = byte_stream.filter_map(|event_result| async move {
            match event_result {
                Ok(event) => match parse_openai_sse_data(&event.data) {
                    Ok(Some(chunk)) => Some(Ok(chunk)),
                    Ok(None) => None,
                    Err(e) => Some(Err(e)),
                },
                Err(e) => Some(Err(AdapterError::Network(e.to_string()))),
            }
        });

        Ok(Box::pin(chunk_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{FinishReason, Message, ToolDefinition};
    use crate::types::ModelId;

    #[test]
    fn test_contract_request_serialization() {
        let req = CompletionRequest::new(
            ModelId::new("gpt-4o").unwrap(),
            vec![
                Message::system("You are a helpful assistant."),
                Message::user("Hello!"),
                Message::assistant("Hi there!"),
                Message::tool("call_123", "{\"result\": 42}"),
            ],
        )
        .with_max_tokens(512)
        .with_temperature(0.7)
        .with_tools(vec![ToolDefinition::new(
            "calculator",
            "Perform arithmetic",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "expr": { "type": "string" }
                }
            }),
        )]);

        let wire_req = OpenAIChatRequest::from_normalized(&req);
        let json = serde_json::to_value(&wire_req).unwrap();

        assert_eq!(json["model"], "gpt-4o");
        assert_eq!(json["max_tokens"], 512);
        assert!((json["temperature"].as_f64().unwrap() - 0.7).abs() < 1e-4);

        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a helpful assistant.");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Hello!");
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["content"], "Hi there!");
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "call_123");
        assert_eq!(messages[3]["content"], "{\"result\": 42}");

        let tools = json["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "calculator");
        assert_eq!(tools[0]["function"]["description"], "Perform arithmetic");
    }

    #[test]
    fn test_contract_response_deserialization() {
        let raw_json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello world!",
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "lookup",
                            "arguments": "{\"query\":\"weather\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 15,
                "completion_tokens": 25,
                "total_tokens": 40
            }
        }"#;

        let wire_resp: OpenAIChatResponse = serde_json::from_str(raw_json).unwrap();
        let resp = wire_resp.into_normalized().unwrap();

        assert_eq!(resp.content, "Hello world!");
        assert_eq!(resp.input_tokens, 15);
        assert_eq!(resp.output_tokens, 25);
        assert_eq!(resp.finish_reason, FinishReason::ToolCalls);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "call_abc");
        assert_eq!(resp.tool_calls[0].name, "lookup");
        assert_eq!(resp.tool_calls[0].arguments["query"], "weather");
    }

    #[test]
    fn test_contract_streaming_sse_fixture() {
        let fixture = [
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}",
            "data: {\"choices\":[{\"delta\":{\"content\":\" \"}}]}",
            "data: {\"choices\":[{\"delta\":{\"content\":\"world!\"}}]}",
            "data: [DONE]",
        ];

        let mut chunks = Vec::new();
        for line in fixture {
            let data = line.strip_prefix("data: ").unwrap();
            if let Some(chunk) = parse_openai_sse_data(data).unwrap() {
                chunks.push(chunk);
            }
        }

        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].delta, "Hello");
        assert!(!chunks[0].done);
        assert_eq!(chunks[1].delta, " ");
        assert!(!chunks[1].done);
        assert_eq!(chunks[2].delta, "world!");
        assert!(!chunks[2].done);
        assert_eq!(chunks[3].delta, "");
        assert!(chunks[3].done);
    }

    #[test]
    fn test_error_status_mapping() {
        let auth_err = map_openai_http_error(401, "invalid key", None);
        assert!(matches!(auth_err, AdapterError::AuthenticationFailed(_)));

        let authz_err = map_openai_http_error(403, "forbidden", None);
        assert!(matches!(authz_err, AdapterError::AuthorizationFailed(_)));

        let rate_err = map_openai_http_error(429, "too many requests", Some(Duration::from_secs(3)));
        assert!(matches!(rate_err, AdapterError::RateLimit { retry_after: Some(d) } if d == Duration::from_secs(3)));

        let svc_err = map_openai_http_error(503, "service unavailable", None);
        assert!(matches!(svc_err, AdapterError::ServiceUnavailable));

        let bad_req = map_openai_http_error(400, "bad request", None);
        assert!(matches!(bad_req, AdapterError::InvalidRequest(_)));
    }
}
