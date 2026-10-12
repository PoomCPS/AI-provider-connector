//! Anthropic Messages API adapter implementation.

pub mod wire;

use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::BoxStream;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use crate::adapter::{AdapterError, ProviderAdapter};
use crate::protocol::{CompletionRequest, CompletionResponse, StreamChunk};
use crate::secret_store::SecretStore;
use crate::types::{ApiKey, EndpointUrl, ProviderKind};
use wire::{
    map_anthropic_http_error, parse_anthropic_sse_event, AnthropicMessagesRequest,
    AnthropicMessagesResponse,
};

/// Adapter communicating with Anthropic Messages API (Claude models).
pub struct AnthropicAdapter {
    client: reqwest::Client,
    endpoint: EndpointUrl,
    secret_store: Arc<dyn SecretStore>,
}

impl AnthropicAdapter {
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
            .get(ProviderKind::Anthropic)
            .map_err(|e| AdapterError::AuthenticationFailed(format!("failed to fetch key from store: {e}")))?
            .ok_or_else(|| AdapterError::AuthenticationFailed("no API key set for Anthropic".into()))
    }

    fn build_url(&self) -> String {
        let base = self.endpoint.as_str().trim_end_matches('/');
        if base.ends_with("/messages") {
            base.to_string()
        } else {
            format!("{}/messages", base)
        }
    }

    fn extract_retry_after(headers: &HeaderMap) -> Option<Duration> {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
    }

    fn build_headers(&self, api_key: &ApiKey) -> Result<HeaderMap, AdapterError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            reqwest::header::HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static("2023-06-01"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(api_key.expose_secret())
                .map_err(|e| AdapterError::AuthenticationFailed(format!("invalid header: {e}")))?,
        );
        Ok(headers)
    }
}

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
        let api_key = self.get_api_key()?;
        let url = self.build_url();
        let payload = AnthropicMessagesRequest::from_normalized(request);
        let headers = self.build_headers(&api_key)?;

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
            return Err(map_anthropic_http_error(status, &body, retry_after));
        }

        let body: AnthropicMessagesResponse = response
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
        let mut payload = AnthropicMessagesRequest::from_normalized(request);
        payload.stream = true;
        let headers = self.build_headers(&api_key)?;

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
            return Err(map_anthropic_http_error(status, &body, retry_after));
        }

        let byte_stream = response.bytes_stream().eventsource();
        let chunk_stream = byte_stream.filter_map(|event_result| async move {
            match event_result {
                Ok(event) => match parse_anthropic_sse_event(Some(&event.event), &event.data) {
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
    fn test_contract_system_and_user_request_serialization() {
        let req = CompletionRequest::new(
            ModelId::new("claude-3-5-sonnet-20241022").unwrap(),
            vec![
                Message::system("You are Claude."),
                Message::user("Hello Claude!"),
            ],
        )
        .with_max_tokens(1024)
        .with_temperature(0.5)
        .with_tools(vec![ToolDefinition::new(
            "lookup",
            "Lookup query",
            serde_json::json!({ "type": "object" }),
        )]);

        let wire_req = AnthropicMessagesRequest::from_normalized(&req);
        let json = serde_json::to_value(&wire_req).unwrap();

        assert_eq!(json["model"], "claude-3-5-sonnet-20241022");
        assert_eq!(json["system"], "You are Claude.");
        assert_eq!(json["max_tokens"], 1024);
        assert!((json["temperature"].as_f64().unwrap() - 0.5).abs() < 1e-4);

        // System message must NOT be in the messages array
        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello Claude!");

        let tools = json["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "lookup");
        assert_eq!(tools[0]["description"], "Lookup query");
        assert!(tools[0]["input_schema"].is_object());
    }

    #[test]
    fn test_contract_anthropic_response_deserialization() {
        let raw_json = r#"{
            "id": "msg_01X9bV3L",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "Sure, I can help with that."
                },
                {
                    "type": "tool_use",
                    "id": "toolu_01A09q90",
                    "name": "lookup",
                    "input": {
                        "topic": "rust"
                    }
                }
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 30,
                "output_tokens": 18
            }
        }"#;

        let wire_resp: AnthropicMessagesResponse = serde_json::from_str(raw_json).unwrap();
        let resp = wire_resp.into_normalized().unwrap();

        assert_eq!(resp.content, "Sure, I can help with that.");
        assert_eq!(resp.input_tokens, 30);
        assert_eq!(resp.output_tokens, 18);
        assert_eq!(resp.finish_reason, FinishReason::ToolCalls);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "toolu_01A09q90");
        assert_eq!(resp.tool_calls[0].name, "lookup");
        assert_eq!(resp.tool_calls[0].arguments["topic"], "rust");
    }

    #[test]
    fn test_contract_anthropic_sse_fixture() {
        let events = [
            ("message_start", "{\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-3-5-sonnet\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}"),
            ("content_block_start", "{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}"),
            ("content_block_delta", "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}"),
            ("content_block_delta", "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" Anthropic\"}}"),
            ("content_block_stop", "{\"type\":\"content_block_stop\",\"index\":0}"),
            ("message_delta", "{\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":10}}"),
            ("message_stop", "{\"type\":\"message_stop\"}"),
        ];

        let mut chunks = Vec::new();
        for (event, data) in events {
            if let Some(chunk) = parse_anthropic_sse_event(Some(event), data).unwrap() {
                chunks.push(chunk);
            }
        }

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].delta, "Hello");
        assert!(!chunks[0].done);
        assert_eq!(chunks[1].delta, " Anthropic");
        assert!(!chunks[1].done);
        assert_eq!(chunks[2].delta, "");
        assert!(chunks[2].done);
    }

    #[test]
    fn test_anthropic_status_code_mapping() {
        let auth_err = map_anthropic_http_error(401, "unauthorized", None);
        assert!(matches!(auth_err, AdapterError::AuthenticationFailed(_)));

        let authz_err = map_anthropic_http_error(403, "forbidden", None);
        assert!(matches!(authz_err, AdapterError::AuthorizationFailed(_)));

        let rate_err = map_anthropic_http_error(429, "rate limited", Some(Duration::from_secs(5)));
        assert!(matches!(rate_err, AdapterError::RateLimit { retry_after: Some(d) } if d == Duration::from_secs(5)));

        let overloaded_err = map_anthropic_http_error(529, "overloaded", None);
        assert!(matches!(overloaded_err, AdapterError::ServiceUnavailable));

        let svc_err = map_anthropic_http_error(503, "unavailable", None);
        assert!(matches!(svc_err, AdapterError::ServiceUnavailable));

        let bad_req = map_anthropic_http_error(400, "bad request", None);
        assert!(matches!(bad_req, AdapterError::InvalidRequest(_)));
    }
}
